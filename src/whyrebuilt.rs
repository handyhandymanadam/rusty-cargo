//! Feature 9a: why did cargo rebuild a crate? Cargo says it itself, in two places: the `Dirty` / `Fresh` lines that `-v` prints and the
//! fingerprint log (`CARGO_LOG=cargo::core::compiler::fingerprint=info`). Both are read, condensed to one record per matching unit, and
//! kept out of the normal text. When a unit is rebuilt only because its settings changed (no `Dirty` line, the old fingerprint is simply
//! not there), the new and the previous fingerprint files of the same crate are compared to name the field that differs.
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Default, Clone)]
struct Unit { version: String, verdict: String, reason: Option<String>, log: Vec<String>, missing_fp: Vec<String> }

#[derive(Default)]
pub struct WhyRebuilt { names: Vec<String>, units: BTreeMap<String, Unit>, pub added_verbose: bool }

fn norm(s: &str) -> String { s.replace('-', "_") }

impl WhyRebuilt {
    pub fn new(names: Vec<String>) -> WhyRebuilt { WhyRebuilt { names, ..Default::default() } }
    pub fn active(&self) -> bool { !self.names.is_empty() }

    /// returns true when the line was ours to consume
    pub fn status(&mut self, verb: &str, rest: &str) -> bool {
        match verb {
            "Fresh" | "Dirty" | "Compiling" | "Checking" => {
                let mut it = rest.split_whitespace();
                let (Some(name), Some(ver)) = (it.next(), it.next()) else { return false };
                let u = self.units.entry(name.to_string()).or_default();
                u.version = ver.trim_start_matches('v').to_string();
                match verb {
                    "Fresh" => u.verdict = "fresh".into(),
                    "Dirty" => { u.verdict = "rebuilt".into(); u.reason = rest.split_once("): ").or_else(|| rest.split_once(": ")).map(|x| x.1.to_string()); }
                    _ => if u.verdict != "rebuilt" { u.verdict = "rebuilt".into(); },
                }
                // our own -v adds Fresh/Dirty lines; Compiling lines stay in the normal text
                self.added_verbose && matches!(verb, "Fresh" | "Dirty")
            }
            "Running" => self.added_verbose && rest.starts_with('`'),
            _ => false,
        }
    }

    /// a CARGO_LOG line: "   0.010s  INFO prepare_target{force=false package_id=NAME vVER (src) target="T"}: cargo::core::compiler::fingerprint: MSG"
    pub fn log_line(&mut self, line: &str) -> bool {
        let t = line.trim_start();
        let Some((time, rest)) = t.split_once(' ') else { return false };
        if !time.ends_with('s') || time[..time.len() - 1].parse::<f64>().is_err() { return false; }
        let rest = rest.trim_start();
        let (level, rest) = rest.split_once(' ').unwrap_or((rest, ""));
        if !matches!(level, "INFO" | "DEBUG" | "TRACE" | "WARN") { return false; }
        let Some(a) = rest.find("package_id=") else { return true };
        let pk = &rest[a + 11..]; let name = pk.split_whitespace().next().unwrap_or("").to_string();
        let msg = rest.split("cargo::core::compiler::fingerprint: ").nth(1).unwrap_or("").trim_end();
        if level == "INFO" && !msg.is_empty() {
            let u = self.units.entry(name).or_default();
            if let Some(i) = msg.find("failed to read `") { if msg.trim_start().starts_with("err:") { let p = &msg[i + 16..]; if let Some(e) = p.find('`') { u.missing_fp.push(p[..e].to_string()); } } }
            // raw Rust Debug dumps (`dirty: FsStatusOutdated(StaleItem(ChangedFile { ... FileTime { ... } }))`), the file paths already in the reason, and
            // the reference file line ("(vs) ...") are not kept
            let m = msg.trim_start();
            if m.starts_with("FileTime") || m.starts_with("dirty:") || m.starts_with("stale:") || m.starts_with("(vs)") { return true; }
            if msg.starts_with("fingerprint dirty") || msg.starts_with("fingerprint error") || msg.trim_start().starts_with("err:") { return true; } // the verdict lines add nothing the reason does not say
            if u.log.len() < 12 { u.log.push(msg.trim_start().to_string()); }
        }
        true
    }

    fn matches(&self, name: &str) -> bool { self.names.iter().any(|n| norm(n) == norm(name)) }

    pub fn records(&self) -> Vec<Value> {
        let mut out = vec![];
        for q in &self.names {
            let hits: Vec<(&String, &Unit)> = self.units.iter().filter(|(n, _)| norm(n) == norm(q)).collect();
            let mut units = vec![];
            for (n, u) in &hits {
                let (kind, mut details) = classify(u);
                let reason = u.reason.as_deref().map(|r| tidy_reason(r, &mut details));
                let mut j = json!({"package": n, "version": u.version, "verdict": if u.verdict.is_empty() { "unknown" } else { &u.verdict }, "reason": reason, "reason_kind": kind, "details": details, "fingerprint_log": u.log});
                if kind == "dependency-rebuilt" {
                    let mut chain = vec![]; let mut cur = details["dependency"].as_str().map(|s| s.to_string()); let mut guard = 0;
                    while let Some(d) = cur { guard += 1; if guard > 20 { break; }
                        let du = self.units.iter().find(|(k, _)| norm(k) == norm(&d)).map(|(_, v)| v);
                        let (dk, dd) = du.map(classify).unwrap_or(("unknown", json!({})));
                        let mut dd = dd; let dr = du.and_then(|x| x.reason.as_deref()).map(|r| tidy_reason(r, &mut dd));
                        chain.push(json!({"package": d, "reason": dr, "reason_kind": dk, "details": dd}));
                        cur = if dk == "dependency-rebuilt" { dd["dependency"].as_str().map(|s| s.to_string()) } else { None };
                    }
                    j["caused_by"] = chain.into();
                }
                if kind == "no-previous-fingerprint" { if let Some(d) = fingerprint_diff(&u.missing_fp) { j["fingerprint_diff"] = d; } }
                units.push(j);
            }
            out.push(json!({"schema": 1, "type": "why-rebuilt", "query": q, "found": !units.is_empty(), "units": units,
                "note": "from cargo's own Dirty/Fresh lines and its fingerprint log; Fresh means cargo reused the previous build"}));
        }
        let _ = self.matches("");
        out
    }
}

/// cargo's reason ends in raw clock values: "the file `src/main.rs` has changed (1792026576.67s, 76000403ns after last build at 1792026576.59s)".
/// Cut the parenthesis when it starts with a number (the clock values differ on every run).
fn tidy_reason(r: &str, _details: &mut Value) -> String {
    let Some(i) = r.rfind(" (") else { return r.to_string() };
    let inner = r[i + 2..].trim_end_matches(')');
    if !inner.chars().next().is_some_and(|c| c.is_ascii_digit()) { return r.to_string(); }
    r[..i].to_string()
}

/// (kind, details) from cargo's own wording of the reason
fn classify(u: &Unit) -> (&'static str, Value) {
    let Some(r) = &u.reason else {
        return match u.verdict.as_str() { "fresh" => ("fresh", json!({})), "rebuilt" => ("no-previous-fingerprint", json!({"meaning": "cargo had no earlier build of this unit with these settings (compiler flags, features, profile or target differ from every earlier build, or it was never built)"})), _ => ("unknown", json!({})) };
    };
    let tick = |s: &str| -> Option<String> { let a = s.find('`')?; let b = s[a + 1..].find('`')?; Some(s[a + 1..a + 1 + b].to_string()) };
    if r.starts_with("the file `") { return ("file-changed", json!({"file": tick(r)})); }
    if r.contains("environment variable") { return ("env-changed", json!({"variable": tick(r)})); }
    if r.contains("was rebuilt") || r.contains("dependency") && r.contains("changed") { return ("dependency-rebuilt", json!({"dependency": tick(r).or_else(|| r.split_whitespace().find(|w| w.chars().next().is_some_and(|c| c.is_alphabetic()) && !matches!(*w, "the" | "a" | "dependency" | "was" | "rebuilt" | "has" | "changed")).map(|s| s.to_string()))})); }
    if r.contains("rustflags") { return ("rustflags-changed", json!({})); }
    if r.contains("profile") { return ("profile-changed", json!({})); }
    if r.contains("features") { return ("features-changed", json!({})); }
    if r.contains("config") { return ("config-changed", json!({})); }
    ("other", json!({}))
}

/// the missing fingerprint is `.../.fingerprint/NAME-HASH/FILE`; the same crate's other directories hold the earlier builds
fn fingerprint_diff(missing: &[String]) -> Option<Value> {
    let first = PathBuf::from(missing.first()?);
    let dir = first.parent()?; let file = first.file_name()?.to_string_lossy().into_owned();
    let fpdir = dir.parent()?; let me = dir.file_name()?.to_string_lossy().into_owned();
    let stem = me.rsplit_once('-')?.0.to_string();
    let json_name = format!("{file}.json");
    let new: Value = serde_json::from_slice(&std::fs::read(dir.join(&json_name)).ok()?).ok()?;
    let mut others: Vec<(std::time::SystemTime, PathBuf)> = vec![];
    for e in std::fs::read_dir(fpdir).ok()?.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n != me && n.rsplit_once('-').is_some_and(|(s, h)| s == stem && h.len() == 16) {
            let p = e.path().join(&json_name);
            if let Ok(m) = std::fs::metadata(&p).and_then(|m| m.modified()) { others.push((m, p)); }
        }
    }
    others.sort();
    let old: Value = serde_json::from_slice(&std::fs::read(&others.last()?.1).ok()?).ok()?;
    let mut diffs = vec![];
    for (k, v) in new.as_object()? {
        if matches!(k.as_str(), "hash" | "deps" | "local" | "path" | "memoized_hash") { continue; }
        let o = old.get(k).cloned().unwrap_or(Value::Null);
        if &o != v { diffs.push(json!({"field": k, "old": o, "new": v})); }
    }
    Some(json!({"compared_with": "the most recent earlier fingerprint of the same crate", "fields": diffs}))
}
