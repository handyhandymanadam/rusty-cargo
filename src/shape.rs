//! The last step before a record is written: stable-mode cleaning (no thread ids, temp paths, clocks) and, later, the detail level.
use serde_json::{Map, Value};

/// How much a record says (rule A of the token budget). `normal` is the default in agent mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Detail { Brief, #[default] Normal, Full }

impl Detail {
    pub fn parse(s: &str) -> Option<Detail> { match s { "brief" => Some(Detail::Brief), "normal" => Some(Detail::Normal), "full" => Some(Detail::Full), _ => None } }
}

#[derive(Clone, Debug, Default)]
pub struct Shape { pub detail: Detail, pub stable: bool, pub fix_preview: bool, pub cwd: Option<String> }

impl Shape {
    pub fn new(cfg: &crate::config::Config) -> Shape {
        Shape { detail: cfg.detail, stable: cfg.stable, fix_preview: cfg.fix_preview, cwd: std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()) }
    }
}

/// Removes what differs from run to run in text rustc, rustdoc and libtest print: the thread id in
/// `thread 'main' (4242) panicked at`, the `note: run with RUST_BACKTRACE` line (it depends on the environment) and the random
/// directory of a doc test (`/tmp/rustdoctestAb12/` becomes `<tmp>/`).
pub fn clean_text(s: &str) -> String {
    if !s.contains("thread '") && !s.contains("RUST_BACKTRACE") && !s.contains("rustdoctest") { return s.to_string(); }
    let mut kept: Vec<String> = vec![];
    for line in s.split('\n') {
        if line.starts_with("note: run with `RUST_BACKTRACE") { continue; }
        kept.push(strip_tmp_dir(&strip_thread_id(line)));
    }
    kept.join("\n")
}

/// "thread 'main' (123) panicked" -> "thread 'main' panicked"
fn strip_thread_id(line: &str) -> String {
    let Some(a) = line.find("thread '") else { return line.to_string() };
    let rest = &line[a + 8..];
    let Some(b) = rest.find("' (") else { return line.to_string() };
    let after = &rest[b + 3..];
    let digits = after.bytes().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || !after[digits..].starts_with(')') { return line.to_string(); }
    format!("{}{}'{}", &line[..a + 8], &rest[..b], &after[digits + 1..])
}

/// "/tmp/rustdocteste1AbC/doctest_bundle_2024.rs" -> "<tmp>/doctest_bundle_2024.rs"
fn strip_tmp_dir(line: &str) -> String {
    let Some(i) = line.find("rustdoctest") else { return line.to_string() };
    let start = line[..i].rfind(|c: char| c.is_whitespace() || c == '\'' || c == '"' || c == '(').map(|p| p + 1).unwrap_or(0);
    let end = line[i..].find('/').map(|p| i + p + 1);
    match end { Some(e) => format!("{}<tmp>/{}", &line[..start], &line[e..]), None => line.to_string() }
}

fn clean_strings(v: &mut Value) {
    match v {
        Value::String(s) => { let c = clean_text(s); if c != *s { *s = c; } }
        Value::Array(a) => a.iter_mut().for_each(clean_strings),
        Value::Object(m) => m.values_mut().for_each(clean_strings),
        _ => {}
    }
}

/// "/x/target/debug/deps/docfail-0b545617a620a0c6" -> "docfail" (the directory and the hash differ per machine and per checkout)
fn stable_binary_path(p: &str) -> String {
    let name = p.rsplit('/').next().unwrap_or(p);
    match name.rsplit_once('-') { Some((n, h)) if h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit()) => n.to_string(), _ => name.to_string() }
}

/// The record as it is written, or `None` when it is left out at this detail level (a passing test at `normal` and `brief`).
pub fn finish(mut v: Value, sh: &Shape) -> Option<Value> {
    if sh.stable { stable(&mut v); }
    if sh.detail == Detail::Full { return Some(v); }
    let ty = v["type"].as_str().unwrap_or("").to_string();
    match ty.as_str() {
        "diagnostic" => if sh.detail == Detail::Brief { v = brief_diag(&v) } else { diag_normal(&mut v, sh) },
        "test" => { if matches!(v["status"].as_str(), Some("ok" | "ignored")) { return None; } test_normal(&mut v, sh.detail); }
        "test-suite" => suite_normal(&mut v),
        "summary" => summary_normal(&mut v, sh.detail),
        "panic-report" => panic_normal(&mut v, sh.detail),
        "why-feature" => { remove(&mut v, &["tree", "rendered", "command"]); if sh.detail == Detail::Brief { remove(&mut v, &["features", "packages", "defined_but_off"]); } }
        "why-rebuilt" => why_rebuilt_normal(&mut v, sh.detail),
        "missing-crate" => missing_normal(&mut v, sh.detail),
        "fix-preview" => fix_normal(&mut v, sh.detail),
        _ => {}
    }
    drop_empty(&mut v);
    Some(v)
}

fn remove(v: &mut Value, keys: &[&str]) { if let Some(m) = v.as_object_mut() { for k in keys { m.shift_remove(*k); } } }

/// removes null, "", [] and {} members (an absent field means empty). `text` is kept: an empty replacement text is a deletion.
fn drop_empty(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for (_, x) in m.iter_mut() { drop_empty(x); }
            m.retain(|k, x| k == "text" || !(x.is_null() || x.as_str().is_some_and(str::is_empty) || x.as_array().is_some_and(Vec::is_empty) || x.as_object().is_some_and(|o| o.is_empty())));
        }
        Value::Array(a) => a.iter_mut().for_each(drop_empty),
        _ => {}
    }
}

/// removes members that are false / 0 (their absence means the same)
fn drop_defaults(v: &mut Value, keys: &[&str]) {
    if let Some(m) = v.as_object_mut() { m.retain(|k, x| !(keys.contains(&k.as_str()) && (x == &Value::Bool(false) || x.as_u64() == Some(0)))); }
}

const MORE: &str = "--agent-detail=full shows everything";

/// keeps the first `n` of an array and records how many were left out as `<key>_omitted`
fn cap(m: &mut Map<String, Value>, key: &str, n: usize) -> bool {
    let Some(Value::Array(a)) = m.get_mut(key) else { return false };
    if a.len() <= n { return false; }
    let extra = a.len() - n; a.truncate(n);
    m.insert(format!("{key}_omitted"), extra.into());
    true
}

fn s_of<'a>(v: &'a Value, k: &str) -> &'a str { v[k].as_str().unwrap_or("") }

fn diag_normal(v: &mut Value, sh: &Shape) {
    let Some(m) = v.as_object_mut() else { return };
    for k in ["rendered", "files", "symbols"] { m.shift_remove(k); }
    if m.get("root").and_then(|r| r.as_str()).is_some_and(|r| sh.cwd.as_deref() == Some(r)) { m.shift_remove("root"); }
    if m.get("suppressed").and_then(|x| x.as_u64()) == Some(0) { m.shift_remove("suppressed"); }
    // test failures: the test record already has the panic
    if m.get("code").and_then(|c| c.as_str()) == Some("R0400") { m.shift_remove("panic"); }
    // follow-ons: no rendered text, at most 5
    let mut more = false;
    if let Some(Value::Array(fo)) = m.get_mut("follow_on") { for f in fo.iter_mut() { remove(f, &["rendered"]); } }
    more |= cap(m, "follow_on", 5);
    // suggestions: fixits are indexes into the top-level fixits; with --fix-preview the preview carries the edits
    if sh.fix_preview {
        m.shift_remove("fixits");
        if let Some(Value::Array(sg)) = m.get_mut("suggestions") { for s in sg.iter_mut() { remove(s, &["fixits"]); } }
    } else if let Some(Value::Array(fx)) = m.get_mut("fixits") { for f in fx.iter_mut() { remove(f, &["applicability", "suggestion"]); } }
    // a type mismatch read from the primary label says the same as that label
    let mut tm_from_label = false;
    if let Some(tm) = m.get_mut("type_mismatch") {
        remove(tm, &["location"]);
        tm_from_label = tm["source"] == "label" && tm["heuristic"] == false;
        if tm["source"] == "label" { remove(tm, &["source"]); }
        if tm["heuristic"] == false { remove(tm, &["heuristic"]); }
    }
    // locations: notes already name the place of note-role locations; labels of type mismatches repeat expected/found
    let notes: Vec<String> = m.get("notes").and_then(|n| n.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    let full_locs: Vec<Value> = m.get("locations").and_then(|l| l.as_array()).cloned().unwrap_or_default();
    // ownership events point at locations by index instead of repeating them
    let mut protected = vec![false; full_locs.len()];
    if let Some(Value::Object(ot)) = m.get_mut("ownership_trace") {
        if let Some(Value::Array(ev)) = ot.get_mut("events") {
            for e in ev.iter_mut() {
                let l = e["location"].clone();
                let idx = full_locs.iter().position(|x| x["file"] == l["file"] && x["line"] == l["line"] && x["column"] == l["column"]);
                if let Some(i) = idx { protected[i] = true; e["loc"] = i.into(); if e["label"] == full_locs[i]["label"] { remove(e, &["label"]); } remove(e, &["location"]); }
                else if let Some(lo) = e.get_mut("location").and_then(|x| x.as_object_mut()) { if lo.get("end_line") == lo.get("line") { lo.shift_remove("end_line"); } }
                remove(e, &["order", "macro"]);
                if e["source"] == "span" { remove(e, &["source"]); }
                if e["heuristic"] == false { remove(e, &["heuristic"]); }
            }
        }
        if ot["complete"] == true { ot.shift_remove("complete"); }
    }
    let mut keep: Vec<usize> = vec![];
    for (i, l) in full_locs.iter().enumerate() {
        let at = format!("{}:{}:{}: ", s_of(l, "file"), l["line"], l["column"]);
        let covered = l["role"] == "note" && notes.iter().any(|n| n.starts_with(&at));
        if protected[i] || !covered { keep.push(i); }
    }
    // at most 8 locations (the protected ones stay)
    let mut kept = 0; keep.retain(|i| { if protected[*i] { return true; } kept += 1; kept <= 8 });
    let omitted_locs = full_locs.len() - keep.len();
    let remap: Vec<Option<usize>> = (0..full_locs.len()).map(|i| keep.iter().position(|k| *k == i)).collect();
    let mut new_locs: Vec<Value> = vec![];
    for i in &keep {
        let mut l = full_locs[*i].clone();
        if l["end_line"] == l["line"] { remove(&mut l, &["end_line"]); if l["end_column"] == l["column"] { remove(&mut l, &["end_column"]); } }
        remove(&mut l, &["expansion"]); // the macro name stays; the call chain (often std's own macros) is in `full`
        if tm_from_label && l["role"] == "primary" && i == &0 { remove(&mut l, &["label"]); }
        new_locs.push(l);
    }
    m.insert("locations".into(), new_locs.into());
    if let Some(Value::Object(ot)) = m.get_mut("ownership_trace") {
        if let Some(Value::Array(ev)) = ot.get_mut("events") { for e in ev.iter_mut() { if let Some(i) = e["loc"].as_u64() { e["loc"] = remap[i as usize].unwrap_or(0).into(); } } }
    }
    // the note-role locations dropped above are not "omitted": the notes carry them
    let really_cut = full_locs.iter().enumerate().filter(|(i, l)| !keep.contains(i) && !(l["role"] == "note" && notes.iter().any(|n| n.starts_with(&format!("{}:{}:{}: ", s_of(l, "file"), l["line"], l["column"]))))).count();
    if really_cut > 0 { m.insert("locations_omitted".into(), really_cut.into()); more = true; }
    let _ = omitted_locs;
    more |= cap(m, "notes", 4);
    more |= cap(m, "suggestions", 3);
    if more { m.insert("more".into(), MORE.into()); }
    drop_empty(v);
}

/// brief: code, message, one location, one fix, counts
fn brief_diag(v: &Value) -> Value {
    let mut o = Map::new();
    for k in ["schema", "type", "code", "severity", "message", "rustc_code", "flag"] { if !v[k].is_null() { o.insert(k.into(), v[k].clone()); } }
    if let Some(p) = v["locations"].as_array().and_then(|a| a.iter().find(|l| l["role"] == "primary")) {
        o.insert("location".into(), serde_json::json!({"file": p["file"], "line": p["line"], "column": p["column"]}));
    }
    if let Some(s) = v["suggestions"].as_array().and_then(|a| a.iter().find(|s| s["fixits"].as_array().is_some_and(|f| !f.is_empty()))) {
        let edits: Vec<Value> = s["fixits"].as_array().unwrap().iter().filter_map(|i| i.as_u64().and_then(|i| v["fixits"].get(i as usize)))
            .map(|f| serde_json::json!({"file": f["file"], "line": f["start"]["line"], "column": f["start"]["column"], "end_line": f["end"]["line"], "end_column": f["end"]["column"], "text": f["text"]})).collect();
        o.insert("fix".into(), serde_json::json!({"message": s["message"], "applicability": s["applicability"], "edits": edits}));
    }
    for (src, dst) in [("notes", "notes"), ("suggestions", "suggestions")] {
        let n = v[src].as_array().map(|a| a.len()).unwrap_or(0);
        if n > 0 { o.insert(dst.into(), n.into()); }
    }
    if let Some(n) = v["suppressed"].as_u64().filter(|n| *n > 0) { o.insert("suppressed".into(), n.into()); }
    if let Some(t) = v["test"].as_str() { o.insert("test".into(), t.into()); }
    if v["backtrace_in"].is_string() { o.insert("backtrace_in".into(), v["backtrace_in"].clone()); }
    Value::Object(o)
}

fn slim_binary(v: &mut Value) {
    if let Some(Value::Object(b)) = v.get_mut("binary") {
        b.shift_remove("path");
        if b.get("unit_target") == b.get("package") { b.shift_remove("unit_target"); }
        if b.get("package") == b.get("target") { b.shift_remove("package"); }
    }
}

fn test_normal(v: &mut Value, detail: Detail) {
    let name = v["name"].as_str().map(String::from);
    remove(v, &["rendered", "duration_ms"]);
    drop_defaults(v, &["should_panic", "output_truncated"]);
    slim_binary(v);
    if let Some(Value::Object(p)) = v.get_mut("panic") {
        // the R0400 diagnostic that goes with a failed test carries the place; the thread is the test's own name
        for k in ["file", "line", "column", "file_is_temporary"] { p.shift_remove(k); }
        if p.get("thread") == name.as_ref().map(|n| Value::from(n.as_str())).as_ref() { p.shift_remove("thread"); }
        // the left/right values are in `assertion`: the message keeps its first line
        if p.contains_key("assertion") {
            if let Some(Value::String(msg)) = p.get_mut("message") { if let Some(i) = msg.find('\n') { msg.truncate(i); } }
            if let Some(Value::Object(a)) = p.get_mut("assertion") { a.shift_remove("custom"); }
        }
    }
    if detail == Detail::Brief {
        let msg = v["panic"]["message"].as_str().map(String::from);
        let at = v["panic"].clone();
        remove(v, &["panic", "output", "binary", "should_panic"]);
        if let Some(mm) = v.as_object_mut() {
            if let Some(x) = msg { mm.insert("message".into(), x.into()); }
            if let (Some(f), Some(l)) = (at["file"].as_str(), at["line"].as_u64()) { mm.insert("at".into(), format!("{f}:{l}").into()); }
        }
    }
}

fn suite_normal(v: &mut Value) {
    slim_binary(v);
    let total: u64 = ["passed", "failed", "ignored", "measured"].iter().map(|k| v[*k].as_u64().unwrap_or(0)).sum();
    if v["expected"].as_u64() == Some(total) { remove(v, &["expected"]); }
    if v["complete"] == true { remove(v, &["complete"]); }
    drop_defaults(v, &["failed", "ignored", "measured", "filtered_out"]);
}

fn summary_normal(v: &mut Value, detail: Detail) {
    remove(v, &["command", "status_lines_consumed"]);
    let clean = v["exit"] == 0 && v["diagnostics"]["errors"].as_u64().unwrap_or(0) == 0 && v["diagnostics"]["warnings"].as_u64().unwrap_or(0) == 0 && v["diagnostics"]["fatal"].as_u64().unwrap_or(0) == 0;
    if let Some(Value::Object(u)) = v.get_mut("units") {
        // success is cheap: no unit list. Otherwise only the units that failed or were compiled (fresh ones are a count), at most 8.
        if clean || detail == Detail::Brief { u.shift_remove("times"); }
        else {
            if let Some(Value::Array(t)) = u.get_mut("times") {
                t.retain(|x| x["fresh"] != true);
                t.sort_by_key(|x| (x["failed"] != true) as u8);
            }
            cap(u, "times", 8);
        }
        if u.get("failed").and_then(|f| f.as_array()).is_some_and(|f| !f.is_empty()) && u.get("times").is_some() { u.shift_remove("failed"); } // the failed units are marked in times
        for k in ["compiled", "fresh"] { if u.get(k).and_then(|x| x.as_u64()) == Some(0) { u.shift_remove(k); } }
    }
    if let Some(Value::Object(d)) = v.get_mut("diagnostics") {
        d.retain(|_, x| x.as_u64() != Some(0));
        for k in ["by_lint", "by_rustc_code", "by_code"] {
            if detail == Detail::Brief { d.shift_remove(k); continue; }
            if let Some(Value::Object(mm)) = d.get_mut(k) {
                let mut items: Vec<(String, Value)> = std::mem::take(mm).into_iter().collect();
                items.sort_by(|a, b| b.1.as_u64().cmp(&a.1.as_u64()).then(a.0.cmp(&b.0)));
                let extra = items.len().saturating_sub(10);
                for (k2, x) in items.into_iter().take(10) { mm.insert(k2, x); }
                if extra > 0 { mm.insert("omitted".into(), extra.into()); }
            }
        }
    }
    if let Some(Value::Object(s)) = v.get_mut("siblings") { s.retain(|_, x| x.as_u64() != Some(0)); }
    if let Some(Value::Object(t)) = v.get_mut("tests") { t.retain(|_, x| x.as_u64() != Some(0)); }
    if v["human_lines_dropped"].as_u64() == Some(0) { remove(v, &["human_lines_dropped"]); }
}

fn panic_normal(v: &mut Value, detail: Detail) {
    remove(v, &["rendered", "trimmed_to"]);
    if v["backtrace_found"] == true { remove(v, &["backtrace_found"]); }
    if v["source"] == "test" { remove(v, &["message", "thread", "location"]); } // the test record and its R0400 diagnostic have them
    if let Some(m) = v.as_object_mut() {
        let shown = m.get("frames").and_then(|f| f.as_array()).map(|a| a.len()).unwrap_or(0);
        if m.get("frames_shown").and_then(|x| x.as_u64()) == Some(shown as u64) { m.shift_remove("frames_shown"); }
        let limit = if detail == Detail::Brief { 3 } else { 12 };
        if cap(m, "frames", limit) { m.insert("more".into(), MORE.into()); }
    }
}

fn why_rebuilt_normal(v: &mut Value, detail: Detail) {
    remove(v, &["note"]);
    if let Some(Value::Array(us)) = v.get_mut("units") {
        for u in us.iter_mut() {
            if u["verdict"] == "fresh" { remove(u, &["reason_kind", "details", "fingerprint_log"]); }
            if detail == Detail::Brief { remove(u, &["details", "fingerprint_log", "caused_by", "fingerprint_diff"]); }
        }
    }
}

fn missing_normal(v: &mut Value, detail: Detail) {
    remove(v, &["network"]);
    if v["path"] == v["name"] { remove(v, &["path"]); }
    if let Some(Value::Object(f)) = v.get_mut("found") { f.retain(|_, x| x != &Value::Bool(false)); }
    if v["add_command"].is_string() { remove(v, &["suggestions"]); }
    if detail == Detail::Brief { remove(v, &["found", "features", "near_misses", "manifest", "cargo_toml_line", "section", "package", "location"]); }
}

fn fix_normal(v: &mut Value, detail: Detail) {
    remove(v, &["apply", "written", "rounds"]);
    if let Some(Value::Array(fs)) = v.get_mut("files") {
        for f in fs.iter_mut() {
            if f["diff_with_unsafe"] == f["diff"] { remove(f, &["diff_with_unsafe"]); }
            drop_defaults(f, &["safe_edits", "unsafe_edits", "conflicts"]);
            if detail == Detail::Brief { remove(f, &["diff_with_unsafe", "root"]); }
        }
    }
    // the diff shows every edit that is in it; the list is only for the edits that are not
    if let Some(Value::Array(es)) = v.get_mut("edits") {
        es.retain(|e| e["in_diff"] != true);
        for e in es.iter_mut() { remove(e, &["in_diff"]); if e["in_diff_with_unsafe"] == false { remove(e, &["in_diff_with_unsafe"]); } }
    }
    if detail == Detail::Brief { remove(v, &["edits"]); }
}

fn stable(v: &mut Value) {
    clean_strings(v);
    let Some(m) = v.as_object_mut() else { return };
    if let Some(Value::Object(b)) = m.get_mut("binary") { if let Some(Value::String(p)) = b.get_mut("path") { *p = stable_binary_path(p); } }
    match m.get("type").and_then(|t| t.as_str()) {
        Some("summary") => {
            m.shift_remove("elapsed_ms");
            if let Some(Value::Array(times)) = m.get_mut("units").and_then(|u| u.get_mut("times")) { for t in times { if let Some(o) = t.as_object_mut() { o.shift_remove("ms"); } } }
        }
        Some("test-suite") => { m.shift_remove("finished_in_s"); }
        _ => {}
    }
}
