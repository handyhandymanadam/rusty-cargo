//! Feature 7: libtest's text (stable libtest has no JSON output) turned into `test` and `test-suite` records. The text itself
//! still goes to standard output unchanged. Handles unit tests, integration tests, doc tests, several binaries, `--nocapture`,
//! `--show-output`, `--ignored`, benches. Per-test durations do not exist in stable libtest, so `duration_ms` is null.
use serde_json::{json, Value};

#[derive(Clone, Debug, Default)]
pub struct Binary { pub kind: String, pub target: String, pub path: String }

#[derive(Debug, Default, Clone)]
pub struct TestCase { pub should_panic: bool, pub name: String, pub status: String, pub reason: Option<String>, pub bench: Option<(String, String)>, pub output: Option<String> }

#[derive(Debug, Default)]
pub struct Suite { pub expected: usize, pub tests: Vec<TestCase>, pub result: Option<Value> }

#[derive(Default)]
pub struct TestReport {
    pub binaries: Vec<Binary>,
    suites: Vec<Suite>,
    cur: Option<Suite>,
    section: Option<(String, Vec<String>)>, // name, lines of "---- name stdout ----"
    in_failures: bool,
    pending: Option<String>,                // "test x ... " seen, result comes on a later line (--nocapture)
    pub seen: bool,
}

pub fn running(rest: &str) -> Option<Binary> {
    // "unittests src/lib.rs (target/debug/deps/x-hash)", "tests/it.rs (path)", "benches/b.rs (path)"
    let (head, path) = match rest.rfind(" (") { Some(i) if rest.ends_with(')') => (&rest[..i], rest[i + 2..rest.len() - 1].to_string()), _ => (rest, String::new()) };
    let (kind, target) = if let Some(t) = head.strip_prefix("unittests ") { ("unittests", t) }
        else if head.starts_with("tests/") { ("integration", head) } else if head.starts_with("benches/") { ("bench", head) }
        else if head.starts_with("examples/") { ("example", head) } else if head.starts_with('`') { return None } else { ("other", head) };
    Some(Binary { kind: kind.into(), target: target.into(), path })
}

impl TestReport {
    pub fn doc_tests(&mut self, name: &str) { self.binaries.push(Binary { kind: "doc-tests".into(), target: name.trim().into(), path: String::new() }); }
    pub fn running_binary(&mut self, rest: &str) { if let Some(b) = running(rest) { self.binaries.push(b); } }

    pub fn line(&mut self, raw: &[u8]) {
        let line = String::from_utf8_lossy(raw); let line = line.trim_end_matches(['\n', '\r']);
        if self.cur.is_none() {
            if let Some(n) = line.strip_prefix("running ").and_then(|r| r.split_whitespace().next()).and_then(|n| n.parse::<usize>().ok()) {
                if line.ends_with(" test") || line.ends_with(" tests") { self.cur = Some(Suite { expected: n, ..Default::default() }); self.seen = true; self.in_failures = false; }
            }
            return;
        }
        // inside a suite
        if let Some(p) = self.pending.take() {
            if let Some(st) = status_word(line.trim()) { self.cur.as_mut().unwrap().tests.push(case(&p, &st)); return; }
            self.pending = Some(p);
        }
        if let Some(rest) = line.strip_prefix("test result: ") { self.finish_section(); self.end_suite(rest); return; }
        if line == "failures:" || line == "successes:" { self.finish_section(); self.in_failures = true; return; }
        if let Some(name) = line.strip_prefix("---- ").and_then(|l| l.strip_suffix(" stdout ----")) { self.finish_section(); self.section = Some((name.to_string(), vec![])); return; }
        if let Some((_, lines)) = &mut self.section { lines.push(line.to_string()); return; }
        if self.in_failures { return; } // the bare list of names after the sections
        if let Some(rest) = line.strip_prefix("test ") {
            if let Some(i) = rest.find(" ... ") {
                let (name, after) = (&rest[..i], &rest[i + 5..]);
                match status_word(after) { Some(st) => self.cur.as_mut().unwrap().tests.push(case(name, &st)), None => self.pending = Some(name.to_string()) }
            }
        }
    }

    fn finish_section(&mut self) {
        if let Some((name, lines)) = self.section.take() {
            let mut text = lines.join("\n"); while text.ends_with('\n') { text.pop(); }
            if let Some(c) = self.cur.as_mut() {
                if let Some(t) = c.tests.iter_mut().find(|t| t.name == name) { t.output = Some(text); }
                else { let mut t = TestCase { name, status: "unknown".into(), ..Default::default() }; t.output = Some(text); c.tests.push(t); }
            }
        }
    }

    fn end_suite(&mut self, rest: &str) {
        let Some(mut s) = self.cur.take() else { return };
        // "FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"
        let (res, nums) = rest.split_once(". ").unwrap_or((rest, ""));
        let mut o = json!({"result": res, "passed": 0, "failed": 0, "ignored": 0, "measured": 0, "filtered_out": 0, "finished_in_s": null});
        for part in nums.split(';') {
            let p = part.trim();
            if let Some(t) = p.strip_prefix("finished in ") { o["finished_in_s"] = t.trim_end_matches('s').parse::<f64>().map(Value::from).unwrap_or(Value::Null); continue; }
            if let Some((n, what)) = p.split_once(' ') {
                let key = match what { "passed" => "passed", "failed" => "failed", "ignored" => "ignored", "measured" => "measured", "filtered out" => "filtered_out", _ => continue };
                o[key] = n.parse::<u64>().unwrap_or(0).into();
            }
        }
        s.result = Some(o); self.suites.push(s);
    }


    /// the records, with suites matched to the binaries cargo announced (both lists keep their own order)
    pub fn records(&mut self, stable: bool, unit_of: &dyn Fn(&str) -> Option<(String, String)>, locals: Option<&crate::panicreport::Locals>, emit_tests: bool) -> (Vec<Value>, Value) {
        if let Some(mut c) = self.cur.take() { c.result = None; self.suites.push(c); } // cut off (killed): keep what we saw
        self.finish_section();
        let mut out = vec![]; let mut panic_ids = 0u32; let (mut passed, mut failed, mut ignored, mut filtered) = (0u64, 0u64, 0u64, 0u64);
        let suites = std::mem::take(&mut self.suites);
        for (i, s) in suites.into_iter().enumerate() {
            let bin = self.binaries.get(i).cloned();
            let (pkg, tgt) = match bin.as_ref() { Some(b) if b.kind == "doc-tests" => (Some(b.target.clone()), Some(b.target.clone())), Some(b) => unit_of(&b.path).unzip(), None => (None, None) };
            let bj = bin.as_ref().map(|b| json!({"kind": b.kind, "target": b.target, "path": if b.path.is_empty() { Value::Null } else { b.path.clone().into() }, "package": pkg, "unit_target": tgt})).unwrap_or(Value::Null);
            let mut tests = s.tests; if stable { tests.sort_by(|a, b| a.name.cmp(&b.name)); }
            for t in &tests {
                let mut r = json!({"schema": 1, "type": "test", "binary": bj, "name": t.name, "status": t.status, "should_panic": t.should_panic, "duration_ms": null});
                if let Some(x) = &t.reason { r["ignore_reason"] = x.clone().into(); }
                if let Some((ns, dev)) = &t.bench { r["bench"] = json!({"ns_per_iter": ns, "deviation": dev}); }
                if let Some(text) = &t.output {
                    let (panic, captured) = parse_panic(text);
                    if let Some(p) = panic { r["panic"] = p; }
                    let (cap, trunc) = trim_output(&captured);
                    r["output"] = cap.into(); r["output_truncated"] = trunc.into(); r["rendered"] = text.clone().into();
                } else { r["output"] = Value::Null; }
                // with --panic-report the backtrace lives in the panic-report record only; the test record and the R0400 diagnostic point to it by id
                let mut pr: Option<Value> = None;
                if let (Some(l), Some(text)) = (locals, &t.output) {
                    if let Some(h) = text.lines().find(|x| x.starts_with("thread '") && x.contains("panicked at ")) {
                        let msg = r["panic"]["message"].as_str().unwrap_or("");
                        if let Some(mut rec) = crate::panicreport::record("test", Some(&t.name), h, msg, text, l) {
                            panic_ids += 1; let id = format!("p{panic_ids}");
                            let mut with_id = serde_json::Map::new();
                            with_id.insert("schema".into(), 1.into()); with_id.insert("type".into(), "panic-report".into()); with_id.insert("id".into(), id.clone().into());
                            if let Some(o) = rec.as_object() { for (k, x) in o { if k != "schema" && k != "type" { with_id.insert(k.clone(), x.clone()); } } }
                            rec = Value::Object(with_id);
                            if rec["backtrace_found"] == true {
                                if let Some(x) = r["rendered"].as_str() { r["rendered"] = cut_backtrace(x).into(); }
                                r["backtrace_in"] = id.into();
                            }
                            pr = Some(rec);
                        }
                    }
                }
                if emit_tests { out.push(r.clone()); }
                if emit_tests && t.status == "failed" { out.push(failed_diag(&r, &bj)); }
                if let Some(rec) = pr { out.push(rec); }
            }
            let res = s.result.clone();
            if let Some(r) = &res { passed += r["passed"].as_u64().unwrap_or(0); failed += r["failed"].as_u64().unwrap_or(0); ignored += r["ignored"].as_u64().unwrap_or(0); filtered += r["filtered_out"].as_u64().unwrap_or(0); }
            let mut su = json!({"schema": 1, "type": "test-suite", "binary": bj, "expected": s.expected, "complete": res.is_some()});
            if let Some(r) = res { for (k, v) in r.as_object().unwrap() { su[k] = v.clone(); } }
            if emit_tests { out.push(su); }
        }
        let n = out.iter().filter(|r| r["type"] == "test-suite").count();
        (out, json!({"suites": n, "passed": passed, "failed": failed, "ignored": ignored, "filtered_out": filtered}))
    }
}

/// the text before the line "stack backtrace:" (the panic message and its location stay)
fn cut_backtrace(text: &str) -> String {
    let mut kept: Vec<&str> = vec![];
    for l in text.split('\n') { if l.trim() == "stack backtrace:" { break; } kept.push(l); }
    kept.join("\n").trim_end().to_string()
}

fn status_word(s: &str) -> Option<String> {
    let s = s.trim();
    if s == "ok" { return Some("ok".into()); }
    if s == "FAILED" { return Some("failed".into()); }
    if s == "ignored" || s.starts_with("ignored, ") { return Some(s.to_string()); }
    if s.starts_with("bench: ") { return Some(s.to_string()); }
    None
}

fn case(name: &str, st: &str) -> TestCase {
    // a #[should_panic] test is listed as "name - should panic"
    let (name, should_panic) = match name.strip_suffix(" - should panic") { Some(n) => (n, true), None => (name, false) };
    let mut t = TestCase { name: name.to_string(), should_panic, ..Default::default() };
    if let Some(r) = st.strip_prefix("ignored, ") { t.status = "ignored".into(); t.reason = Some(r.to_string()); }
    else if st == "ignored" { t.status = "ignored".into(); }
    else if let Some(b) = st.strip_prefix("bench: ") {
        t.status = "bench".into();
        let ns = b.split_whitespace().next().unwrap_or("").replace(',', "");
        let dev = b.split("(+/- ").nth(1).and_then(|x| x.split(')').next()).unwrap_or("").replace(',', "");
        t.bench = Some((ns, dev));
    } else { t.status = st.to_string(); }
    t
}

/// (panic info, text before the panic message = what the test printed)
pub fn parse_panic(text: &str) -> (Option<Value>, String) {
    let lines: Vec<&str> = text.lines().collect();
    let Some(i) = lines.iter().position(|l| l.starts_with("thread '") && l.contains("panicked at ")) else {
        // a #[should_panic] test that did not panic: "note: test did not panic as expected at src/lib.rs:16:8"
        if let Some(l) = lines.iter().find(|l| l.contains("test did not panic as expected")) {
            let at = l.split(" at ").nth(1).unwrap_or("");
            let mut p = at.rsplitn(3, ':'); let (c, ln, f) = (p.next().and_then(|x| x.parse::<u64>().ok()), p.next().and_then(|x| x.parse::<u64>().ok()), p.next());
            return (Some(json!({"thread": null, "message": "test did not panic as expected", "file": f, "line": ln, "column": c, "kind": "did-not-panic"})), String::new());
        }
        return (None, text.to_string());
    };
    let head = lines[i];
    let name = head.strip_prefix("thread '").and_then(|r| r.split("' ").next()).unwrap_or("").to_string();
    let at = head.split("panicked at ").nth(1).unwrap_or("").trim_end_matches(':');
    let mut parts = at.rsplitn(3, ':');
    let (col, line, file) = (parts.next(), parts.next(), parts.next());
    let (file, line, col) = match (file, line.and_then(|x| x.parse::<u64>().ok()), col.and_then(|x| x.parse::<u64>().ok())) { (Some(f), Some(l), Some(c)) => (Some(f.to_string()), Some(l), Some(c)), _ => (None, None, None) };
    let mut msg: Vec<&str> = vec![];
    for l in &lines[i + 1..] {
        if l.starts_with("note: run with `RUST_BACKTRACE") || *l == "stack backtrace:" { break; }
        msg.push(l);
    }
    while msg.last().is_some_and(|l| l.trim().is_empty()) { msg.pop(); }
    let message = msg.join("\n");
    let mut p = json!({"thread": name, "message": message, "file": file, "line": line, "column": col});
    // doc tests run from a temporary file: the real place is the code block named in the test name
    if file.as_deref().is_some_and(|f| f.contains("/rustdoctest")) { p["file_is_temporary"] = true.into(); }
    if let Some(a) = assertion(&msg) { p["assertion"] = a; }
    let before = lines[..i].join("\n");
    (Some(p), before)
}

fn assertion(msg: &[&str]) -> Option<Value> {
    let first = *msg.first()?;
    let kind = if first.starts_with("assertion `left == right` failed") { "left == right" } else if first.starts_with("assertion `left != right` failed") { "left != right" }
        else if let Some(e) = first.strip_prefix("assertion failed: ") { return Some(json!({"kind": "assert", "expr": e})); } else { return None };
    let custom = first.split_once("failed: ").map(|x| x.1.to_string());
    let find = |k: &str| msg.iter().find_map(|l| l.trim_start().strip_prefix(k).map(|v| v.to_string()));
    Some(json!({"kind": kind, "custom": custom, "left": find("left: "), "right": find("right: ")}))
}

fn trim_output(s: &str) -> (String, bool) {
    let t = s.trim_matches('\n');
    if t.len() <= 4000 { return (t.to_string(), false); }
    let mut end = 4000; while !t.is_char_boundary(end) { end -= 1; }
    (format!("{}\n... (trimmed)", &t[..end]), true)
}

fn failed_diag(t: &Value, bin: &Value) -> Value {
    let msg = t["panic"]["message"].as_str().and_then(|m| m.lines().next()).unwrap_or("test failed");
    let p = &t["panic"];
    let name = t["name"].as_str().unwrap_or("");
    let real_panic = p["file"].as_str().filter(|_| p["file_is_temporary"] != true).zip(p["line"].as_u64()).zip(p["column"].as_u64());
    let mut locs: Vec<Value> = vec![]; let mut files: Vec<Value> = vec![];
    // a doc test is located by its code block (the line in its name): a panic inside the library code it called is not the test's place
    // (the old output pointed there and looked like another test). Both are given: the block as primary, the panic site as secondary.
    let doc = if bin["kind"] == "doc-tests" { doctest_place(name) } else { None };
    if let Some((f, l)) = &doc {
        locs.push(json!({"file": f, "line": l, "column": 1, "end_line": l, "end_column": 1, "role": "primary", "label": "start of the failing doc test"}));
        files.push(f.clone().into());
    }
    if let Some(((f, l), c)) = real_panic {
        let role = if doc.is_some() { "secondary" } else { "primary" };
        locs.push(json!({"file": f, "line": l, "column": c, "end_line": l, "end_column": c, "role": role, "label": "panicked here"}));
        if !files.iter().any(|x| x == f) { files.push(f.into()); }
    }
    let mut d = json!({"schema": 1, "type": "diagnostic", "code": "R0400", "severity": "error", "message": format!("test failed: {}: {}", t["name"].as_str().unwrap_or(""), msg),
           "rustc_code": null, "flag": null, "package": bin["package"], "target": {"name": bin["unit_target"], "kind": bin["kind"]}, "root": null, "files": files,
           "symbols": [t["name"]], "notes": [], "suggestions": [], "locations": locs, "fixits": [], "rendered": t["rendered"], "test": t["name"], "panic": t["panic"]});
    if t["backtrace_in"].is_string() { d["backtrace_in"] = t["backtrace_in"].clone(); }
    d
}

/// "src/lib.rs - inc (line 1)" -> ("src/lib.rs", 1)
fn doctest_place(name: &str) -> Option<(String, u64)> {
    let (file, rest) = name.split_once(" - ")?;
    let l = rest.rsplit_once("(line ")?.1.trim_end_matches(')').parse().ok()?;
    Some((file.to_string(), l))
}
