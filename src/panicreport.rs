//! Feature 10: a panic with its backtrace, cut down to the frames in the user's own crates (the workspace and path dependencies).
//! The full text stays in `rendered`. The flag makes cargo's child run with RUST_BACKTRACE=1 unless you set it yourself; that
//! changes the program's own stderr text (it prints the backtrace), nothing else.
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Default, Clone)]
pub struct Locals { pub crates: BTreeSet<String>, pub roots: Vec<String> }

impl Locals {
    pub fn add(&mut self, package_id: &str, manifest: &str, target: &str) {
        if !package_id.starts_with("path+") { return; }
        self.crates.insert(target.replace('-', "_"));
        if let Some(d) = std::path::Path::new(manifest).parent() { let s = d.to_string_lossy().into_owned(); if !self.roots.contains(&s) { self.roots.push(s); } }
    }
    fn is_user(&self, function: &str, file: Option<&str>) -> bool {
        let f = file.map(|f| f.trim_start_matches("./"));
        if let Some(f) = f {
            if f.starts_with("/rustc/") || f.contains("/.cargo/registry/") || f.contains("/.cargo/git/") || f.contains("/.rustup/") { return false; }
            if !f.starts_with('/') { return true; }
            if self.roots.iter().any(|r| f.starts_with(r.as_str())) { return true; }
        }
        // no file (stripped debug info): the crate name at the start of the path, or as the type in `<crate::T as Trait>`
        let head = function.trim_start_matches('<');
        let first = head.split("::").next().unwrap_or("").split(' ').next().unwrap_or("");
        self.crates.contains(first)
    }
}

#[derive(Clone, Debug)]
pub struct Frame { pub index: u64, pub function: String, pub file: Option<String>, pub line: Option<u64>, pub column: Option<u64> }

/// "   3: panicker::level1\n             at ./src/main.rs:3:28"
pub fn frames(lines: &[&str]) -> Vec<Frame> {
    let mut out: Vec<Frame> = vec![];
    for l in lines {
        let t = l.trim_start();
        if let Some((n, rest)) = t.split_once(": ") {
            if let Ok(i) = n.parse::<u64>() { if l.starts_with(' ') && !rest.starts_with("at ") { out.push(Frame { index: i, function: rest.to_string(), file: None, line: None, column: None }); continue; } }
        }
        if let (Some(at), Some(f)) = (t.strip_prefix("at "), out.last_mut()) {
            let mut p = at.rsplitn(3, ':');
            let (c, ln, file) = (p.next().and_then(|x| x.parse().ok()), p.next().and_then(|x| x.parse().ok()), p.next());
            match (file, ln, c) { (Some(fl), Some(ln), Some(c)) => { f.file = Some(fl.to_string()); f.line = Some(ln); f.column = Some(c); } _ => { f.file = Some(at.to_string()); } }
        }
    }
    out
}

pub fn record(source: &str, test: Option<&str>, header: &str, message: &str, text: &str, locals: &Locals) -> Option<Value> {
    let lines: Vec<&str> = text.lines().collect();
    let bt = lines.iter().position(|l| l.trim() == "stack backtrace:");
    let fr = match bt { Some(i) => frames(&lines[i + 1..]), None => vec![] };
    // header: "thread 'main' (pid) panicked at inner/src/lib.rs:2:17:"
    let thread = header.strip_prefix("thread '").and_then(|r| r.split("' ").next()).map(|s| s.to_string());
    let at = header.split("panicked at ").nth(1).unwrap_or("").trim_end_matches(':');
    let mut p = at.rsplitn(3, ':'); let (col, line, file) = (p.next().and_then(|x| x.parse::<u64>().ok()), p.next().and_then(|x| x.parse::<u64>().ok()), p.next());
    let total = fr.len();
    let user: Vec<Value> = fr.iter().filter(|f| locals.is_user(&f.function, f.file.as_deref())).map(|f| json!({"index": f.index, "function": f.function,
        "file": f.file.as_deref().map(|x| x.trim_start_matches("./")), "line": f.line, "column": f.column})).collect();
    Some(json!({"schema": 1, "type": "panic-report", "source": source, "test": test, "thread": thread, "message": message,
        "location": {"file": file, "line": line, "column": col}, "backtrace_found": bt.is_some(), "frames_total": total, "frames_shown": user.len(),
        "frames": user, "trimmed_to": "frames in this workspace's crates and path dependencies", "rendered": text}))
}

/// Reads a program's stderr line by line and finds panics; used for `cargo run`.
#[derive(Default)]
pub struct Scanner { cur: Option<(String, Vec<String>)>, done: Vec<Value> }

impl Scanner {
    pub fn line(&mut self, raw: &[u8], locals: &Locals) {
        let l = String::from_utf8_lossy(raw); let l = l.trim_end_matches(['\n', '\r']);
        if l.starts_with("thread '") && l.contains("panicked at ") { self.flush(locals); self.cur = Some((l.to_string(), vec![l.to_string()])); return; }
        let Some((_, lines)) = &mut self.cur else { return };
        let in_bt = lines.iter().any(|x| x.trim() == "stack backtrace:");
        let frame_like = { let t = l.trim_start(); t.starts_with("at ") || t.split_once(": ").is_some_and(|(n, _)| n.parse::<u64>().is_ok()) };
        if l.trim() == "stack backtrace:" || (in_bt && frame_like) || !in_bt && !l.starts_with("note: Some details") { lines.push(l.to_string()); }
        else { if l.starts_with("note: ") { lines.push(l.to_string()); } self.flush(locals); }
    }
    pub fn flush(&mut self, locals: &Locals) {
        if let Some((header, lines)) = self.cur.take() {
            let msg: Vec<&str> = lines[1..].iter().map(|s| s.as_str()).take_while(|l| l.trim() != "stack backtrace:" && !l.starts_with("note: run with")).collect();
            let message = msg.join("\n").trim_end().to_string();
            if let Some(r) = record("run", None, &header, &message, &lines.join("\n"), locals) { self.done.push(r); }
        }
    }
    pub fn take(&mut self, locals: &Locals) -> Vec<Value> { self.flush(locals); std::mem::take(&mut self.done) }
}
