//! Feature 4: ownership and lifetime errors as a trace: where the value was declared, moved or borrowed, what conflicts,
//! and where it is used again. Everything comes from the labels and spans rustc already puts in its JSON (no rustc patch).
//! Roles are recognised from rustc's label wording, so each event says whether it was `heuristic`; a relation the JSON
//! does not carry is named in `missing`.
use crate::diag::Diag;
use serde_json::{json, Value};

struct Kind { name: &'static str, expect: &'static [&'static str] }

fn kind_of(code: &str) -> Option<Kind> {
    let (name, expect): (&str, &[&str]) = match code {
        "E0382" => ("use-after-move", &["moved", "use_after_move"]),
        "E0499" => ("two-mutable-borrows", &["borrowed", "conflicting_borrow", "later_use"]),
        "E0502" => ("conflicting-borrow", &["borrowed", "conflicting_borrow", "later_use"]),
        "E0503" => ("use-while-mutably-borrowed", &["borrowed", "conflicting_use"]),
        "E0505" => ("move-while-borrowed", &["borrowed", "conflicting_move", "later_use"]),
        "E0506" => ("assign-while-borrowed", &["borrowed", "conflicting_assign", "later_use"]),
        "E0597" => ("borrowed-value-dropped", &["borrowed_value", "dropped", "later_use"]),
        "E0716" => ("temporary-dropped-while-borrowed", &["temporary_created", "temporary_freed", "later_use"]),
        "E0515" => ("reference-to-local-returned", &["return_of_local"]),
        "E0373" => ("closure-may-outlive-borrow", &[]),
        "E0507" => ("move-out-of-borrow", &[]),
        "E0596" => ("mutable-borrow-of-immutable", &[]),
        "E0594" => ("assign-to-immutable", &[]),
        "E0384" => ("assign-twice-to-immutable", &[]),
        "E0381" => ("use-of-uninitialized", &[]),
        "E0521" => ("borrowed-data-escapes", &[]),
        "E0621" | "E0623" | "E0106" | "E0261" | "E0700" => ("lifetime-mismatch", &[]),
        _ => return None,
    };
    Some(Kind { name, expect })
}

/// (role, heuristic) from one label; `primary` tells which side of the conflict rustc points at
fn role_of(label: &str, primary: bool) -> (&'static str, bool) {
    let l = label;
    let has = |s: &str| l.contains(s);
    if has("move occurs because") { return ("declared", false); }
    if has("declared here") { return ("declared", false); }
    if has("value moved here") || has("value moved into closure here") || has("value partially moved here") { return ("moved", false); }
    if has("later used here") || has("later used by call") || has("later borrowed here") || has("later stored here") || has("later captured here") || has("borrow might be used here") || has("later dropped here") { return ("later_use", false); }
    if has("after move") || has("after being moved") { return ("use_after_move", false); }
    if has("dropped here while still borrowed") { return ("dropped", false); }
    if has("temporary value is freed") { return ("temporary_freed", false); }
    if has("creates a temporary value which is freed while still in use") || has("creates a temporary") { return ("temporary_created", false); }
    if has("borrowed value does not live long enough") { return ("borrowed_value", false); }
    if has("returns a value referencing data owned by the current function") || has("returns a reference to data owned by the current function") || has("returns a value referencing") { return ("return_of_local", false); }
    if has("is assigned to here but it was already borrowed") || has("assigned to here but it was already borrowed") { return ("conflicting_assign", false); }
    if has("move out of") && has("occurs here") { return (if primary { "conflicting_move" } else { "moved" }, false); }
    if has("borrow of") && has("occurs here") { return (if primary { "conflicting_use" } else { "borrowed" }, false); }
    if has("mutable borrow occurs here") || has("immutable borrow occurs here") || has("borrow occurs here") || has("is borrowed here") {
        return (if primary { "conflicting_borrow" } else { "borrowed" }, false);
    }
    if has("takes ownership") { return ("takes_ownership", false); }
    if has("first assignment") || has("first assignment to") { return ("first_assignment", false); }
    if has("cannot assign twice") { return ("conflicting_assign", false); }
    if primary { ("primary", true) } else { ("related", true) }
}

fn rank(role: &str) -> u8 {
    match role {
        "declared" => 0, "borrowed" | "moved" | "temporary_created" | "borrowed_value" | "first_assignment" => 1,
        "takes_ownership" => 2,
        "conflicting_borrow" | "conflicting_move" | "conflicting_assign" | "conflicting_use" | "use_after_move" | "dropped" | "temporary_freed" | "return_of_local" => 3,
        "later_use" => 4, _ => 2,
    }
}

fn loc(s: &Value) -> Value {
    json!({"file": s["file_name"], "line": s["line_start"], "column": s["column_start"], "end_line": s["line_end"], "end_column": s["column_end"]})
}

pub fn trace(d: &Diag) -> Option<Value> {
    let code = d.rustc_code.as_deref()?;
    let kind = kind_of(code)?;
    if d.msg.is_null() { return None; }
    let mut events: Vec<(u8, u64, u64, Value)> = vec![];
    let mut ty: Option<String> = None;
    let add = |s: &Value, label: &str, source: &str, events: &mut Vec<(u8, u64, u64, Value)>, ty: &mut Option<String>| {
        if !s["suggested_replacement"].is_null() || label.is_empty() { return; }
        let (role, heur) = role_of(label, s["is_primary"] == true);
        if role == "declared" && ty.is_none() {
            if let Some(a) = label.find("has type `") { let r = &label[a + 10..]; if let Some(b) = r.find('`') { *ty = Some(r[..b].to_string()); } }
        }
        let rk = rank(role);
        let (l, c) = (s["line_start"].as_u64().unwrap_or(0), s["column_start"].as_u64().unwrap_or(0));
        events.push((rk, l, c, json!({"role": role, "location": loc(s), "label": label, "source": source, "heuristic": heur, "macro": s["expansion"]["macro_decl_name"]})));
    };
    if let Some(spans) = d.msg["spans"].as_array() {
        for s in spans { add(s, s["label"].as_str().unwrap_or(""), "span", &mut events, &mut ty); }
    }
    // child notes with their own span: "this function takes ownership of the receiver", "borrow later used here"
    fn walk(m: &Value, f: &mut dyn FnMut(&Value, &str)) {
        if let Some(cs) = m["children"].as_array() {
            for c in cs {
                if let Some(spans) = c["spans"].as_array() {
                    let msg = c["message"].as_str().unwrap_or("");
                    for s in spans {
                        let lab = s["label"].as_str().filter(|x| !x.is_empty()).unwrap_or(msg);
                        f(s, lab);
                    }
                }
                walk(c, f);
            }
        }
    }
    let mut child_items: Vec<(Value, String)> = vec![];
    walk(&d.msg, &mut |s, l| child_items.push((s.clone(), l.to_string())));
    for (s, l) in &child_items {
        // only child notes that name a relation; plain "help: consider cloning" have a suggestion and are skipped in add()
        let (role, _) = role_of(l, false);
        if role != "related" { add(s, l, "child-note", &mut events, &mut ty); }
    }
    events.sort_by(|a, b| (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
    // the same span can be reported by the main diagnostic and by a child; keep the first
    let mut seen: Vec<(String, Value)> = vec![];
    let mut out: Vec<Value> = vec![];
    for (_, _, _, mut e) in events {
        let key = (e["role"].as_str().unwrap_or("").to_string(), e["location"].clone());
        if seen.contains(&key) { continue; }
        seen.push(key);
        e["order"] = out.len().into();
        out.push(e);
    }
    let have: Vec<&str> = out.iter().filter_map(|e| e["role"].as_str()).collect();
    let missing: Vec<&str> = kind.expect.iter().copied().filter(|r| !have.contains(r)).collect();
    let variable = d.message.find('`').and_then(|a| d.message[a + 1..].find('`').map(|b| d.message[a + 1..a + 1 + b].to_string()));
    let mut t = json!({"kind": kind.name, "variable": variable, "type": ty, "events": out, "complete": missing.is_empty(), "missing": missing});
    if !missing.is_empty() {
        t["note"] = "rustc's JSON output has no span for the missing relation(s); the text in `rendered` is rustc's own and may show more".into();
    }
    Some(t)
}
