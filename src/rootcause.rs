//! Feature 2: follow-on errors are listed under the first cause instead of being repeated. The followers' own records are
//! not written; the leader lists them (code, message, place, rule) and `suppressed` counts them, and the leader's `rendered`
//! text is rustc's own. `--no-root-cause` writes every record.
use crate::diag::{Diag, Sev};
use serde_json::{json, Value};

pub struct Rec<'a> { pub diag: &'a Diag, pub followers: Vec<(&'a Diag, &'static str)> }

fn place(d: &Diag) -> Option<(&str, u64, u64)> { d.primary.as_ref().map(|p| (p.file.as_str(), p.line, p.col)) }

fn first_symbol(d: &Diag) -> Option<String> {
    let a = d.message.find('`')?; let b = d.message[a + 1..].find('`')?; Some(d.message[a + 1..a + 1 + b].to_string())
}

/// every backticked name in the message and in the span labels
fn mentioned(d: &Diag) -> Vec<String> {
    let mut texts = vec![d.message.clone()];
    if let Some(s) = d.msg["spans"].as_array() { for s in s { if let Some(l) = s["label"].as_str() { texts.push(l.to_string()); } } }
    let mut out = vec![];
    for t in texts {
        let mut rest = t.as_str();
        while let Some(a) = rest.find('`') {
            let Some(b) = rest[a + 1..].find('`') else { break };
            out.push(rest[a + 1..a + 1 + b].to_string()); rest = &rest[a + b + 2..];
        }
    }
    out
}

pub fn group<'a>(diags: &'a [Diag]) -> Vec<Rec<'a>> {
    let mut leaders: Vec<Rec<'a>> = vec![];
    let mut unresolved: Vec<(String, usize)> = vec![]; // (name, leader index) of unresolved-name errors
    'next: for d in diags {
        // same place: identical diagnostic repeated (any severity)
        for l in leaders.iter_mut() {
            if l.diag.code == d.code && l.diag.message == d.message && l.diag.flag == d.flag && place(l.diag) == place(d) && place(d).is_some() {
                l.followers.push((d, "same-place")); continue 'next;
            }
        }
        if d.sev == Sev::Error || d.sev == Sev::Fatal {
            if d.code == "R0001" {
                if let Some(name) = first_symbol(d) {
                    if let Some((_, li)) = unresolved.iter().find(|(n, li)| *n == name && leaders[*li].diag.message == d.message) {
                        leaders[*li].followers.push((d, "same-unresolved-name")); continue 'next;
                    }
                    unresolved.push((name, leaders.len()));
                }
            } else if !unresolved.is_empty() {
                let ments = mentioned(d);
                if let Some((_, li)) = unresolved.iter().find(|(n, _)| ments.iter().any(|m| m == n)) {
                    leaders[*li].followers.push((d, "mentions-unresolved-name")); continue 'next;
                }
            }
        }
        leaders.push(Rec { diag: d, followers: vec![] });
    }
    leaders
}

pub fn follower_json(d: &Diag, rule: &str) -> Value {
    json!({"code": d.code, "severity": d.sev.as_str(), "message": d.message, "rustc_code": d.rustc_code, "flag": d.flag,
           "location": d.primary.as_ref().map(|p| json!({"file": p.file, "line": p.line, "column": p.col})), "rule": rule, "rendered": d.msg["rendered"]})
}
