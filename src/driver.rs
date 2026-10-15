//! Agent mode: run cargo with `--message-format=json`, read its JSON messages and its text, write shared-format records
//! on stderr. Cargo's own text and the program's / test harness's standard output pass through unchanged.
use crate::config::Config;
use crate::diag::{self, Diag, Sev, Unit};
use crate::rootcause;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Instant;

/// stderr size budget per command at `normal` and `brief` (bytes)
const SOFT_BUDGET: usize = 12_000;
const HARD_BUDGET: usize = 20_000;

const JSON_SUBCOMMANDS: &[&str] = &["build", "b", "check", "c", "clippy", "test", "t", "bench", "run", "r", "doc", "d", "rustc", "rustdoc"];

pub struct Invocation { pub sub: Option<String>, pub sub_index: Option<usize>, pub has_message_format: bool, pub dd_index: Option<usize> }

/// Finds the subcommand the way cargo does: the first argument that is not `+toolchain` and not an option
/// (global options that take a value are skipped with it).
pub fn inspect(args: &[OsString]) -> Invocation {
    let mut inv = Invocation { sub: None, sub_index: None, has_message_format: false, dd_index: None };
    let mut i = 0;
    while i < args.len() {
        let a = args[i].to_string_lossy();
        if inv.dd_index.is_none() {
            if a == "--" { inv.dd_index = Some(i); }
            else if a == "--message-format" || a.starts_with("--message-format=") { inv.has_message_format = true; }
            else if inv.sub.is_none() {
                if matches!(&*a, "-C" | "-Z" | "--config" | "--color") { i += 2; continue; }
                // selection options used without a subcommand (`rusty-cargo --explain-feature=x --features f`)
                if matches!(&*a, "--features" | "-F" | "-p" | "--package" | "--manifest-path" | "--target") { i += 2; continue; }
                if !a.starts_with('+') && !a.starts_with('-') { inv.sub = Some(a.to_string()); inv.sub_index = Some(i); }
            }
        }
        i += 1;
    }
    inv
}

fn is_version(args: &[OsString], inv: &Invocation) -> bool {
    if inv.sub.as_deref() == Some("version") { return true; }
    inv.sub.is_none() && args.iter().take_while(|a| *a != "--").any(|a| a == "--version" || a == "-V")
}

fn is_help(args: &[OsString], inv: &Invocation) -> bool {
    args.iter().take(inv.dd_index.unwrap_or(args.len())).any(|a| a == "--help" || a == "-h") && inv.sub.as_deref() != Some("help")
}

pub struct Out { err: std::io::Stderr, file: Option<std::fs::File>, fd: Option<std::fs::File>, shape: crate::shape::Shape, spent: usize, cut: BTreeMap<String, u64> }
impl Out {
    /// `agent` is whether agent mode is on: only then does RUSTY_AGENT_OUT=<path> add a records file (appended, one record per line,
    /// written as each record is produced). An empty path is refused with a note; a file that cannot be opened is reported once and ignored.
    pub fn new(cfg: &Config) -> Out {
        let mut file = None;
        let mut fd = None;
        let mut fd_problem = None;
        if cfg.agent {
            // RUSTY_AGENT_FD=N: an already-open descriptor gets the same records; a bad one is one warning record (R0500), never a failure
            if let Some(v) = std::env::var_os("RUSTY_AGENT_FD") {
                match open_fd(&v.to_string_lossy()) {
                    Ok(f) => fd = Some(f),
                    Err(why) => fd_problem = Some(format!("RUSTY_AGENT_FD={}: {why}: no records on that descriptor", v.to_string_lossy())),
                }
            }
            if let Some(p) = std::env::var_os("RUSTY_AGENT_OUT") {
                if p.is_empty() { eprintln!("rusty-cargo: RUSTY_AGENT_OUT is empty; no records file"); }
                else {
                    match std::fs::OpenOptions::new().create(true).append(true).open(&p) {
                        Ok(f) => file = Some(f),
                        Err(e) => eprintln!("rusty-cargo: cannot open RUSTY_AGENT_OUT={}: {e}; no records file", std::path::Path::new(&p).display()),
                    }
                }
            }
        }
        let mut out = Out { err: std::io::stderr(), file, fd, shape: crate::shape::Shape::new(cfg), spent: 0, cut: BTreeMap::new() };
        if let Some(msg) = fd_problem {
            out.rec(&json!({"schema": 1, "type": "diagnostic", "code": "R0500", "severity": "warning", "message": msg,
                "suggestions": [{"message": "check the descriptor number (3 or more, open for writing) or unset RUSTY_AGENT_FD"}]}));
        }
        out
    }
    fn to_file(&mut self, line: &[u8]) { for f in [&mut self.file, &mut self.fd].into_iter().flatten() { let _ = f.write_all(line); } }
    pub fn rec(&mut self, v: &Value) {
        let Some(v) = crate::shape::finish(v.clone(), &self.shape) else { return };
        let mut s = serde_json::to_string(&v).unwrap_or_default(); s.push('\n');
        self.to_file(s.as_bytes()); // the records file gets every record; only stderr is held to the size budget
        if self.shape.detail != crate::shape::Detail::Full && v["type"] != "summary" {
            // budget (token-budget rule G): past SOFT_BUDGET only errors and what was asked for go on; past HARD_BUDGET nothing but the summary
            let low = v["type"] == "diagnostic" && matches!(v["severity"].as_str(), Some("warning" | "note"));
            if self.spent + s.len() > HARD_BUDGET || (low && self.spent + s.len() > SOFT_BUDGET) {
                *self.cut.entry(v["type"].as_str().unwrap_or("record").to_string()).or_default() += 1; return;
            }
        }
        self.spent += s.len();
        let mut l = self.err.lock(); let _ = l.write_all(s.as_bytes()); let _ = l.flush();
    }
    /// the records the size budget kept off stderr, by type (for the summary); `None` when nothing was cut
    pub fn cut_info(&self) -> Option<Value> {
        if self.cut.is_empty() { return None; }
        let pointer = if self.file.is_some() && self.fd.is_some() { "all records are in the RUSTY_AGENT_OUT file and the RUSTY_AGENT_FD descriptor" } else if self.fd.is_some() { "all records are in the RUSTY_AGENT_FD descriptor" } else if self.file.is_some() { "all records are in the RUSTY_AGENT_OUT file" } else { "--agent-detail=full shows everything; RUSTY_AGENT_OUT=FILE or RUSTY_AGENT_FD=N keeps every record" };
        Some(json!({"by_type": self.cut, "more": pointer}))
    }
    /// a record written by a sibling tool (rusty-cc, rusty-ld) that passes through: same bytes on stderr, and into the file
    pub fn passthrough_record(&mut self, b: &[u8]) { self.to_file(b); self.raw(b); }
    pub fn raw(&mut self, b: &[u8]) { let mut l = self.err.lock(); let _ = l.write_all(b); let _ = l.flush(); }
}

/// a descriptor the caller left open, as a new append handle on the same file, pipe or terminal (the caller's descriptor is not touched, no unsafe)
fn open_fd(value: &str) -> Result<std::fs::File, String> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) { return Err("not a decimal number".into()); }
    let n: u64 = value.parse().map_err(|_| "number too large".to_string())?;
    if n < 3 { return Err(format!("{n} is below 3 (0, 1 and 2 are standard input, output and error)")); }
    match std::fs::read_to_string(format!("/proc/self/fdinfo/{n}")) {
        Ok(t) => {
            let flags = t.lines().find_map(|l| l.strip_prefix("flags:")).and_then(|v| u32::from_str_radix(v.trim(), 8).ok());
            if flags.is_some_and(|f| f & 3 == 0) { return Err(format!("descriptor {n} is open for reading only")); }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(format!("descriptor {n} is not open")),
        Err(_) => {}
    }
    std::fs::OpenOptions::new().append(true).open(format!("/proc/self/fd/{n}")).map_err(|e| { let s = e.to_string(); format!("descriptor {n} cannot be written ({})", s.split(" (os error").next().unwrap_or(&s)) })
}

/// children (cargo and the tools it runs) must not write the descriptor themselves: this process copies their records
pub fn scrub(c: &mut Command) -> &mut Command { c.env_remove("RUSTY_AGENT_FD") }

fn real_cmd(real: &Path) -> Command { let mut c = Command::new(real); c.arg0("cargo"); c }
/// the same for a child that is waited for (never the exec path: an exec'd cargo has no records of ours to copy)
fn waited_cmd(real: &Path) -> Command { let mut c = real_cmd(real); scrub(&mut c); c }

fn exit_code(st: std::process::ExitStatus) -> i32 { st.code().unwrap_or_else(|| 128 + st.signal().unwrap_or(0)) }

/// `--version`: cargo's text unchanged, then the agent-mode line. `--help`: cargo's text, then our section.
fn run_with_trailer(real: &Path, args: &[OsString], trailer: &str) -> i32 {
    let out = scrub(Command::new(real).arg0("cargo").args(args).stdin(Stdio::inherit()).stderr(Stdio::inherit())).output();
    match out {
        Ok(o) => { let mut so = std::io::stdout().lock(); let _ = so.write_all(&o.stdout); if o.status.success() { let _ = so.write_all(trailer.as_bytes()); } let _ = so.flush(); exit_code(o.status) }
        Err(e) => { eprintln!("rusty-cargo: cannot run {}: {e}", real.display()); 127 }
    }
}

fn args_toolchain(args: &[OsString]) -> Option<OsString> { args.iter().take_while(|a| *a != "--").find(|a| a.to_string_lossy().starts_with('+')).cloned() }

pub fn run(cfg: &Config, real: &Path, args: Vec<OsString>) -> i32 {
    let inv = inspect(&args);
    if cfg.agent && is_version(&args, &inv) { return run_with_trailer(real, &args, "agent mode: yes, schema 1\n"); }
    if cfg.agent && is_help(&args, &inv) { return run_with_trailer(real, &args, &crate::help::agent_section()); }
    let sub = inv.sub.clone().unwrap_or_default();
    if !JSON_SUBCOMMANDS.contains(&sub.as_str()) || inv.has_message_format {
        if cfg.why_feature.is_empty() { return plain_exec(cfg, real, &args); }
        // run the command as given, then answer the question
        let code = if sub.is_empty() { 0 } else { match waited_cmd(real).args(&args).status() { Ok(s) => exit_code(s), Err(e) => { eprintln!("rusty-cargo: cannot run {}: {e}", real.display()); return 127; } } };
        let ctx = crate::features::Ctx::new(cfg, real, args_toolchain(&args), &args);
        let mut out = Out::new(cfg);
        for r in ctx.feature_records() { out.rec(&r); }
        return code;
    }
    let mut run_args = args.clone();
    let at = inv.dd_index.unwrap_or(run_args.len());
    run_args.insert(at, OsString::from("--message-format=json"));
    let mut added_v = false;
    if !cfg.why_rebuilt.is_empty() && !args.iter().take(at).any(|a| a == "-v" || a == "-vv" || a == "--verbose" || a == "-q" || a == "--quiet") { run_args.insert(at, OsString::from("-v")); added_v = true; }
    drive(cfg, real, &inv, &args, run_args, added_v)
}

fn plain_exec(cfg: &Config, real: &Path, args: &[OsString]) -> i32 {
    let mut c = real_cmd(real); c.args(args);
    if cfg.agent { c.env("RUSTY_AGENT", "1"); }
    let err = c.exec();
    eprintln!("rusty-cargo: cannot run {}: {err}", real.display()); 127
}

enum Ev { Out(Vec<u8>), Err(Vec<u8>), Done }

fn reader(r: impl std::io::Read + Send + 'static, tx: mpsc::Sender<Ev>, stdout: bool) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut br = BufReader::with_capacity(64 * 1024, r);
        loop {
            let mut line = Vec::new();
            match br.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => { if tx.send(if stdout { Ev::Out(line) } else { Ev::Err(line) }).is_err() { break; } }
            }
        }
        let _ = tx.send(Ev::Done);
    })
}

/// "   Compiling foo v0.1.0 (...)": a cargo status line (verb right-aligned in 12 columns)
pub fn status_line(line: &[u8]) -> Option<(&str, &str)> {
    let s = std::str::from_utf8(line).ok()?.trim_end_matches('\n');
    let t = s.trim_start_matches(' ');
    let lead = s.len() - t.len();
    let (verb, rest) = t.split_once(' ')?;
    if lead + verb.len() != 12 { return None; }
    matches!(verb, "Compiling" | "Checking" | "Fresh" | "Documenting" | "Running" | "Doc-tests" | "Finished" | "Dirty" | "Downloading" | "Downloaded" | "Updating" | "Locking" | "Adding" | "Removing" | "Blocking" | "Building" | "Packaging" | "Verifying" | "Installing" | "Fixed" | "Generated").then_some((verb, rest))
}

#[derive(Default)]
pub struct UnitStat { pub ms: Option<u128>, pub fresh: bool, pub compiled: bool, pub errored: bool }

pub struct State {
    pub diags: Vec<Diag>,    // waiting to be written
    pub counted: Vec<Diag>,  // already written; kept small, for the summary
    pub units: BTreeMap<(String, String, String), UnitStat>, // (package, target, kind)
    pub failed: Vec<String>,
    pub status_consumed: usize, pub human_dropped: usize,
    pub sibling_errors: u64, pub sibling_warnings: u64,
    pub build_ok: Option<bool>,
}

fn unit_key(u: &Unit) -> (String, String, String) { (u.package.clone(), u.target.clone(), u.kind.clone()) }

fn drive(cfg: &Config, real: &Path, inv: &Invocation, orig_args: &[OsString], run_args: Vec<OsString>, added_v: bool) -> i32 {
    let start = Instant::now();
    let mut cmd = waited_cmd(real);
    cmd.args(&run_args).stdin(Stdio::inherit()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if cfg.agent { cmd.env("RUSTY_AGENT", "1"); }
    if cfg.panic_report && std::env::var_os("RUST_BACKTRACE").is_none() { cmd.env("RUST_BACKTRACE", "1"); }
    if !cfg.why_rebuilt.is_empty() { cmd.env("CARGO_LOG", "cargo::core::compiler::fingerprint=info"); }
    if cfg.stable { cmd.env("CARGO_TERM_COLOR", "never").env("CARGO_TERM_PROGRESS_WHEN", "never"); }
    // keep the wrapper alive when the terminal sends ^C: cargo gets the signal too and ends, then we report
    unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN); libc::signal(libc::SIGQUIT, libc::SIG_IGN); }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => { eprintln!("rusty-cargo: cannot run {}: {e}", real.display()); return 127; }
    };
    let (tx, rx) = mpsc::channel();
    let h1 = reader(child.stdout.take().unwrap(), tx.clone(), true);
    let h2 = reader(child.stderr.take().unwrap(), tx, false);
    let mut out = Out::new(cfg);
    let mut st = State { diags: vec![], counted: vec![], units: BTreeMap::new(), failed: vec![], status_consumed: 0, human_dropped: 0, sibling_errors: 0, sibling_warnings: 0, build_ok: None };
    let buffer_diags = cfg.stable || cfg.root_cause || cfg.fix_preview || cfg.ownership || cfg.missing_crate;
    let mut so = std::io::stdout();
    let mut done = 0;
    let mut pending_started: BTreeMap<(String, String), Instant> = BTreeMap::new(); // "Compiling name vX" -> start
    let mut ctx = crate::features::Ctx::new(cfg, real, args_toolchain(orig_args), orig_args);
    ctx.why.added_verbose = added_v;
    while done < 2 {
        let Ok(ev) = rx.recv() else { break };
        match ev {
            Ev::Done => done += 1,
            Ev::Out(line) => {
                if line.starts_with(b"{\"reason\":") {
                    let Ok(v) = serde_json::from_slice::<Value>(&line) else { let _ = so.write_all(&line); continue };
                    match v["reason"].as_str() {
                        Some("compiler-message") => {
                            let parsed = diag::parse(&v);
                            // a unit with an error and no artifact is a unit that failed to compile: the summary lists it too
                            if let Some(d) = &parsed { if matches!(d.sev, Sev::Error | Sev::Fatal) { st.units.entry(unit_key(&d.unit)).or_default().errored = true; } }
                            if !cfg.diag_json {
                                // records not wanted: print what cargo itself would have printed
                                if let Some(r) = v["message"]["rendered"].as_str() { out.raw(r.as_bytes()); }
                                if let Some(d) = parsed { st.counted.push(strip(d)); }
                            } else if let Some(d) = parsed {
                                if buffer_diags { st.diags.push(d); }
                                else { let root = crate::paths::root_for(&d); let rec = diag::record(&d, root.as_deref()); out.rec(&rec); st.counted.push(strip(d)); }
                            }
                        }
                        Some("compiler-artifact") => {
                            let u = Unit::from_msg(&v); let key = unit_key(&u);
                            let e = st.units.entry(key.clone()).or_default();
                            e.fresh = v["fresh"].as_bool().unwrap_or(false); e.compiled = true;
                            if let Some(s) = pending_started.remove(&(u.package.clone(), u.version.clone())) { e.ms = Some(s.elapsed().as_millis()); }
                            ctx.artifact(&v);
                        }
                        Some("build-finished") => {
                            st.build_ok = v["success"].as_bool();
                            if buffer_diags { flush_diags(cfg, &mut st, &mut out, &mut ctx); }
                        }
                        _ => {}
                    }
                    continue;
                }
                ctx.stdout_line(&line, &mut out);
                let _ = so.write_all(&line); let _ = so.flush();
            }
            Ev::Err(line) => {
                if ctx.log_line(&line) { continue; }
                if let Some((verb, rest)) = status_line(&line) {
                    if matches!(verb, "Compiling" | "Checking" | "Documenting") {
                        // "foo v0.1.0 (/path)": start of the unit
                        let mut it = rest.split_whitespace();
                        if let (Some(n), Some(v)) = (it.next(), it.next()) { pending_started.insert((n.to_string(), v.trim_start_matches('v').to_string()), Instant::now()); }
                    }
                    let consumed = ctx.status(verb, rest);
                    if consumed || cfg.stable { st.status_consumed += 1; continue; }
                } else if line.starts_with(b"{\"schema\":1") {
                    if let Ok(v) = serde_json::from_slice::<Value>(&line) {
                        match v["severity"].as_str() { Some("error" | "fatal") => st.sibling_errors += 1, Some("warning") => st.sibling_warnings += 1, _ => {} }
                    }
                    ctx.stderr_line(&line, &mut out);
                    out.passthrough_record(&line);
                    continue;
                } else if let Some(r) = line.strip_prefix(b"error: could not compile `") {
                    if let Some(end) = r.iter().position(|b| *b == b'`') { st.failed.push(String::from_utf8_lossy(&r[..end]).into_owned()); }
                }
                // agent mode only: cargo's closing lines that the records already say (exit code and counts are in `summary`,
                // the failed tests in `test-suite`); they are counted, not hidden silently
                if human_line_is_carried(cfg, &line) { st.human_dropped += 1; continue; }
                ctx.stderr_line(&line, &mut out);
                out.raw(&line);
            }
        }
    }
    let _ = h1.join(); let _ = h2.join();
    let status = child.wait();
    let code = match &status { Ok(s) => exit_code(*s), Err(_) => 1 };
    if buffer_diags { flush_diags(cfg, &mut st, &mut out, &mut ctx); }
    ctx.finish(&mut out, &st);
    if cfg.build_summary {
        let mut sm = summary(cfg, inv, orig_args, &run_args, &st, &ctx, code, start.elapsed().as_millis());
        if let Some(c) = out.cut_info() { sm["records_omitted"] = c; }
        out.rec(&sm);
    }
    code
}

/// cargo's own trailing lines that agent-mode records already carry. Only with the whole umbrella part that carries them on.
fn human_line_is_carried(cfg: &Config, line: &[u8]) -> bool {
    if !cfg.agent || !cfg.build_summary { return false; }
    let l = line.strip_suffix(b"\n").unwrap_or(line);
    if cfg.diag_json {
        if l.starts_with(b"error: could not compile `") { return true; }
        if l == b"warning: build failed, waiting for other jobs to finish..." { return true; }
    }
    if cfg.test_report {
        if l.starts_with(b"error: test failed, to rerun pass ") || l.starts_with(b"error: doctest failed, to rerun pass ") { return true; }
        // "error: 3 targets failed:" and its list "    `--lib`", "    `--test it`", "    `--doc`" (the suites say which failed)
        if l.starts_with(b"error: ") && l.ends_with(b" targets failed:") && l[7..l.len() - 16].iter().all(|b| b.is_ascii_digit()) { return true; }
        if l.starts_with(b"    `--") && l.ends_with(b"`") { return true; }
    }
    false
}

fn flush_diags(cfg: &Config, st: &mut State, out: &mut Out, ctx: &mut crate::features::Ctx) {
    let diags = std::mem::take(&mut st.diags);
    if diags.is_empty() { return; }
    // group per unit, units in a fixed order when stable
    let mut by_unit: Vec<(Unit, Vec<Diag>)> = vec![];
    for d in diags {
        match by_unit.iter_mut().find(|(u, _)| *u == d.unit) { Some((_, v)) => v.push(d), None => by_unit.push((d.unit.clone(), vec![d])) }
    }
    if cfg.stable { by_unit.sort_by(|a, b| (&a.0.package, &a.0.kind, &a.0.target).cmp(&(&b.0.package, &b.0.kind, &b.0.target))); }
    let mut kept: Vec<Diag> = vec![];
    for (_, ds) in &by_unit {
        let recs: Vec<rootcause::Rec> = if cfg.root_cause { rootcause::group(ds) } else { ds.iter().map(|d| rootcause::Rec { diag: d, followers: vec![] }).collect() };
        let mut items: Vec<(Value, (String, u64, u64, String, String), Vec<Value>)> = vec![];
        for r in &recs {
            let root = crate::paths::root_for(r.diag);
            let mut v = diag::record(r.diag, root.as_deref());
            if cfg.root_cause {
                let m = v.as_object_mut().unwrap();
                m.insert("suppressed".into(), r.followers.len().into());
                if !r.followers.is_empty() { m.insert("follow_on".into(), r.followers.iter().map(|(d, rule)| rootcause::follower_json(d, rule)).collect::<Vec<_>>().into()); }
            }
            crate::features::decorate(cfg, r.diag, &mut v);
            let key = r.diag.primary.as_ref().map(|p| (p.file.clone(), p.line, p.col, r.diag.code.to_string(), r.diag.message.clone()))
                .unwrap_or((String::from("\u{10ffff}"), 0, 0, r.diag.code.to_string(), r.diag.message.clone()));
            let extra = ctx.extra_records(r.diag);
            items.push((v, key, extra));
        }
        if cfg.stable { items.sort_by(|a, b| a.1.cmp(&b.1)); }
        for (v, _, extra) in &items { out.rec(v); ctx.diag_record(v); for x in extra { out.rec(x); } }
        for r in &recs {
            for (f, _) in &r.followers { let fr = diag::record(f, crate::paths::root_for(f).as_deref()); ctx.diag_record(&fr); }
            kept.push(strip(r.diag.clone()));
            for (f, _) in &r.followers { kept.push(strip((*f).clone())); }
        }
    }
    st.counted.extend(kept);
}

/// counting needs only the small fields; the big JSON is dropped once the record is written
fn strip(mut d: Diag) -> Diag { d.msg = Value::Null; d }

fn summary(cfg: &Config, inv: &Invocation, orig: &[OsString], run: &[OsString], st: &State, ctx: &crate::features::Ctx, exit: i32, ms: u128) -> Value {
    let _ = (cfg, inv, orig);
    let (mut errors, mut warnings, mut fatal, mut notes) = (0u64, 0u64, 0u64, 0u64);
    let mut by_lint: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_rustc: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_code: BTreeMap<String, u64> = BTreeMap::new();
    for d in &st.counted {
        match d.sev { Sev::Error => errors += 1, Sev::Warning => warnings += 1, Sev::Fatal => fatal += 1, Sev::Note => notes += 1 }
        if let Some(f) = &d.flag { *by_lint.entry(f.clone()).or_default() += 1; }
        if let Some(c) = &d.rustc_code { *by_rustc.entry(c.clone()).or_default() += 1; }
        *by_code.entry(d.code.to_string()).or_default() += 1;
    }
    let (mut compiled, mut fresh) = (0u64, 0u64);
    let mut times = vec![];
    for ((p, t, k), u) in &st.units {
        let failed = u.errored && !u.compiled;
        if failed {} else if u.fresh { fresh += 1; } else { compiled += 1; }
        let mut m = Map::new();
        m.insert("package".into(), p.clone().into()); m.insert("target".into(), t.clone().into()); m.insert("kind".into(), k.clone().into());
        m.insert("fresh".into(), u.fresh.into());
        if failed { m.insert("failed".into(), true.into()); }
        if let Some(x) = u.ms { m.insert("ms".into(), Value::from(x as u64)); } // fresh units have no start time: no ms (was null)
        times.push(Value::Object(m));
    }
    let mut command: Vec<Value> = vec![Value::from("cargo")];
    command.extend(run.iter().map(|a| Value::from(a.to_string_lossy().into_owned())));
    let mut r = json!({"schema": 1, "type": "summary", "mode": "cargo", "exit": exit, "elapsed_ms": ms as u64, "command": command,
        "units": {"compiled": compiled, "fresh": fresh, "failed": st.failed, "times": times},
        "diagnostics": {"errors": errors, "fatal": fatal, "warnings": warnings, "notes": notes, "by_lint": by_lint, "by_rustc_code": by_rustc, "by_code": by_code},
        "siblings": {"errors": st.sibling_errors, "warnings": st.sibling_warnings}});
    if cfg.stable { r["status_lines_consumed"] = st.status_consumed.into(); }
    if cfg.agent { r["human_lines_dropped"] = st.human_dropped.into(); }
    ctx.add_summary(&mut r);
    r
}
