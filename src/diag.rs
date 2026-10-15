//! One cargo `compiler-message` turned into one shared-format `diagnostic` record (features 1, 3 and 12).
use crate::codes;
use serde_json::{json, Map, Value};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sev { Fatal, Error, Warning, Note }

impl Sev {
    pub fn as_str(self) -> &'static str { match self { Sev::Fatal => "fatal", Sev::Error => "error", Sev::Warning => "warning", Sev::Note => "note" } }
}

/// Which compilation unit a message belongs to.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Unit { pub package: String, pub version: String, pub target: String, pub kind: String, pub manifest: String, pub src_path: String }

impl Unit {
    pub fn from_msg(v: &Value) -> Unit {
        let pid = v["package_id"].as_str().unwrap_or("");
        let (package, version) = parse_package_id(pid);
        let t = &v["target"];
        Unit {
            package, version,
            target: t["name"].as_str().unwrap_or("").to_string(),
            kind: t["kind"].as_array().and_then(|k| k.first()).and_then(|k| k.as_str()).unwrap_or("").to_string(),
            manifest: v["manifest_path"].as_str().unwrap_or("").to_string(),
            src_path: t["src_path"].as_str().unwrap_or("").to_string(),
        }
    }
}

/// "path+file:///x/errors3#0.1.0", "path+file:///x/errors3#errors3@0.1.0", "registry+https://...#serde@1.0.0"
fn parse_package_id(pid: &str) -> (String, String) {
    let (head, frag) = pid.rsplit_once('#').unwrap_or((pid, ""));
    if let Some((n, v)) = frag.split_once('@') { return (n.to_string(), v.to_string()); }
    if frag.starts_with(|c: char| c.is_ascii_digit()) {
        let name = head.rsplit('/').next().unwrap_or("").to_string();
        return (name, frag.to_string());
    }
    (frag.to_string(), String::new())
}

/// A diagnostic as read from cargo, with the pieces other features need.
#[derive(Clone, Debug)]
pub struct Diag {
    pub unit: Unit,
    pub msg: Value,           // rustc's diagnostic object
    pub sev: Sev,
    pub rustc_code: Option<String>,
    pub flag: Option<String>,
    pub code: &'static str,
    pub message: String,
    pub primary: Option<Pos>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Pos { pub file: String, pub line: u64, pub col: u64, pub end_line: u64, pub end_col: u64 }

pub fn sev_of(level: &str) -> Option<Sev> {
    match level {
        "error" => Some(Sev::Error),
        "warning" => Some(Sev::Warning),
        "note" | "help" => Some(Sev::Note),
        l if l.starts_with("error: internal compiler error") => Some(Sev::Fatal),
        _ => None, // failure-note and anything unknown: not a diagnostic record
    }
}

fn span_pos(s: &Value) -> Pos {
    Pos { file: s["file_name"].as_str().unwrap_or("").to_string(), line: s["line_start"].as_u64().unwrap_or(0), col: s["column_start"].as_u64().unwrap_or(0),
          end_line: s["line_end"].as_u64().unwrap_or(0), end_col: s["column_end"].as_u64().unwrap_or(0) }
}

pub fn primary_span(m: &Value) -> Option<&Value> {
    m["spans"].as_array()?.iter().find(|s| s["is_primary"].as_bool() == Some(true))
}

pub fn parse(v: &Value) -> Option<Diag> {
    let m = v.get("message")?;
    if m["$message_type"].as_str().is_some_and(|t| t != "diagnostic") { return None; }
    let sev = sev_of(m["level"].as_str()?)?;
    let message = m["message"].as_str().unwrap_or("").to_string();
    let c = m["code"]["code"].as_str().map(|s| s.to_string());
    // rustc puts the lint name where an error has its E-number; split them
    let (rustc_code, flag) = match c {
        Some(c) if is_error_number(&c) => (Some(c), None),
        Some(c) => (None, Some(c)),
        None => (None, None),
    };
    let primary = primary_span(m).map(span_pos);
    let code = codes::classify(sev, rustc_code.as_deref(), flag.as_deref(), &message, primary.is_some());
    Some(Diag { unit: Unit::from_msg(v), msg: m.clone(), sev, rustc_code, flag, code, message, primary })
}

fn is_error_number(c: &str) -> bool { c.len() == 5 && c.starts_with('E') && c[1..].bytes().all(|b| b.is_ascii_digit()) }

fn loc_json(s: &Value, role: &str) -> Value {
    let p = span_pos(s);
    let mut m = Map::new();
    m.insert("file".into(), p.file.into()); m.insert("line".into(), p.line.into()); m.insert("column".into(), p.col.into());
    m.insert("end_line".into(), p.end_line.into()); m.insert("end_column".into(), p.end_col.into()); m.insert("role".into(), role.into());
    m.insert("label".into(), s["label"].as_str().filter(|l| !l.is_empty()).map(|l| Value::from(l)).unwrap_or(Value::Null));
    if let Some(name) = s["expansion"]["macro_decl_name"].as_str() {
        m.insert("macro".into(), name.into());
        // the chain of macro calls this span came out of, innermost first (what rustc prints as "in this macro invocation")
        let mut chain = vec![]; let mut e = &s["expansion"];
        while chain.len() < 8 && e.is_object() {
            let call = &e["span"];
            chain.push(json!({"macro": e["macro_decl_name"], "call_site": {"file": call["file_name"], "line": call["line_start"], "column": call["column_start"]},
                              "def_site": {"file": e["def_site_span"]["file_name"], "line": e["def_site_span"]["line_start"], "column": e["def_site_span"]["column_start"]}}));
            e = &call["expansion"];
        }
        m.insert("expansion".into(), chain.into());
    }
    Value::Object(m)
}

fn pos_json(line: &Value, col: &Value, off: &Value) -> Value { json!({"line": line, "column": col, "offset": off}) }

/// Everything a record needs, pulled out of the (nested) children.
pub struct Parts {
    pub locations: Vec<Value>,
    pub notes: Vec<String>,
    pub suggestions: Vec<Value>,
    pub fixits: Vec<Value>,
    pub files: Vec<String>,
}

fn at(s: &Value) -> String { let p = span_pos(s); format!("{}:{}:{}", p.file, p.line, p.col) }

pub fn parts(d: &Diag) -> Parts {
    let m = &d.msg;
    let mut p = Parts { locations: vec![], notes: vec![], suggestions: vec![], fixits: vec![], files: vec![] };
    let mut add_file = |f: &str, files: &mut Vec<String>| if !f.is_empty() && !files.iter().any(|x| x == f) { files.push(f.to_string()); };
    // main spans: primary first, then the labelled secondary ones
    let spans: Vec<&Value> = m["spans"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    for s in spans.iter().filter(|s| s["is_primary"] == true) { add_file(s["file_name"].as_str().unwrap_or(""), &mut p.files); p.locations.push(loc_json(s, "primary")); }
    for s in spans.iter().filter(|s| s["is_primary"] != true) { add_file(s["file_name"].as_str().unwrap_or(""), &mut p.files); p.locations.push(loc_json(s, "secondary")); }
    // a main span can carry its own inline suggestion ("help: prefix it with an underscore")
    for s in &spans {
        if !s["suggested_replacement"].is_null() {
            let label = s["label"].as_str().unwrap_or("suggested change").to_string();
            add_suggestion(&mut p, &label, std::slice::from_ref(s));
        }
    }
    flatten_children(m, &mut p, &mut add_file);
    p
}

fn flatten_children(m: &Value, p: &mut Parts, add_file: &mut impl FnMut(&str, &mut Vec<String>)) {
    let Some(children) = m["children"].as_array() else { return };
    for c in children {
        let level = c["level"].as_str().unwrap_or("note");
        let msg = c["message"].as_str().unwrap_or("");
        let spans: Vec<&Value> = c["spans"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
        let sugg: Vec<&Value> = spans.iter().copied().filter(|s| !s["suggested_replacement"].is_null()).collect();
        if !sugg.is_empty() {
            add_suggestion_refs(p, msg, &sugg);
            for s in &sugg { add_file(s["file_name"].as_str().unwrap_or(""), &mut p.files); }
        } else {
            let prim = spans.iter().find(|s| s["is_primary"] == true).or(spans.first());
            match prim {
                Some(s) => p.notes.push(format!("{}: {}: {}", at(s), level, msg)),
                None => p.notes.push(format!("{}: {}", level, msg)),
            }
            for s in &spans {
                add_file(s["file_name"].as_str().unwrap_or(""), &mut p.files);
                p.locations.push(loc_json(s, "note"));
            }
        }
        flatten_children(c, p, add_file);
    }
}

fn add_suggestion(p: &mut Parts, message: &str, spans: &[&Value]) { add_suggestion_refs(p, message, spans) }

/// One suggestion = one child (or one inline-suggestion span) = all its edits, to be applied together.
fn add_suggestion_refs(p: &mut Parts, message: &str, spans: &[&Value]) {
    let idx = p.suggestions.len();
    let mut ids = vec![];
    let mut appl = "Unspecified";
    for s in spans {
        let a = s["suggestion_applicability"].as_str().unwrap_or("Unspecified");
        appl = a_worst(appl, a, ids.is_empty());
        ids.push(p.fixits.len());
        p.fixits.push(json!({
            "file": s["file_name"], "start": pos_json(&s["line_start"], &s["column_start"], &s["byte_start"]),
            "end": pos_json(&s["line_end"], &s["column_end"], &s["byte_end"]), "text": s["suggested_replacement"],
            "applicability": a, "suggestion": idx,
        }));
    }
    p.suggestions.push(json!({"message": message, "option": null, "applicability": appl, "fixits": ids}));
}

/// A multi-part suggestion is only as safe as its least safe part.
fn a_worst<'a>(cur: &'a str, new: &'a str, first: bool) -> &'a str {
    fn rank(a: &str) -> u8 { match a { "MachineApplicable" => 0, "MaybeIncorrect" => 1, "HasPlaceholders" => 2, _ => 3 } }
    if first || rank(new) > rank(cur) { new } else { cur }
}

fn symbols(msg: &str) -> Vec<String> {
    let mut out = vec![]; let mut rest = msg;
    while let Some(a) = rest.find('`') {
        let Some(b) = rest[a + 1..].find('`') else { break };
        let s = &rest[a + 1..a + 1 + b];
        if !s.is_empty() && !out.iter().any(|x| x == s) && out.len() < 4 { out.push(s.to_string()); }
        rest = &rest[a + b + 2..];
    }
    out
}

/// Feature 3: expected and found types as fields. Reads rustc's own label ("expected `i32`, found `&str`") first, then the
/// two-line note ("expected struct `String`\n   found reference `&str`"). `heuristic` is true for the note form.
pub fn type_mismatch(d: &Diag) -> Option<Value> {
    if !matches!(d.rustc_code.as_deref(), Some("E0308" | "E0053" | "E0271")) { return None; }
    let prim = primary_span(&d.msg)?;
    if let Some(label) = prim["label"].as_str() {
        if let Some((ek, et, fk, ft)) = expect_found(label) {
            return Some(mismatch_json(ek, et, fk, ft, prim, "label", false));
        }
    }
    // other spans of the main diagnostic, then the notes
    for s in d.msg["spans"].as_array()?.iter() {
        if let Some((ek, et, fk, ft)) = s["label"].as_str().and_then(expect_found) { return Some(mismatch_json(ek, et, fk, ft, prim, "label", true)); }
    }
    for c in d.msg["children"].as_array()? {
        if let Some((ek, et, fk, ft)) = c["message"].as_str().and_then(expect_found) { return Some(mismatch_json(ek, et, fk, ft, prim, "note", true)); }
    }
    None
}

fn mismatch_json(ek: String, et: String, fk: String, ft: String, prim: &Value, source: &str, heuristic: bool) -> Value {
    json!({"expected": et, "found": ft, "expected_kind": if ek.is_empty() { Value::Null } else { ek.into() }, "found_kind": if fk.is_empty() { Value::Null } else { fk.into() },
           "location": loc_json(prim, "primary"), "source": source, "heuristic": heuristic})
}

/// "expected `A`, found `B`" / "expected struct `A`\n found reference `B`" -> (kind, type, kind, type)
pub fn expect_found(text: &str) -> Option<(String, String, String, String)> {
    let e = text.find("expected ")?;
    let (ek, et, after) = kind_and_type(&text[e + "expected ".len()..])?;
    let f = after.find("found ")?;
    let (fk, ft, _) = kind_and_type(&after[f + "found ".len()..])?;
    Some((ek, et, fk, ft))
}

/// "struct `String`, found ..." -> ("struct", "String", rest after closing backtick)
fn kind_and_type(s: &str) -> Option<(String, String, &str)> {
    let a = s.find('`')?;
    let kind = s[..a].trim().to_string();
    if kind.len() > 24 || kind.contains(['\n', ',']) { return None; }
    let rest = &s[a + 1..];
    let b = rest.find('`')?;
    Some((kind, rest[..b].to_string(), &rest[b + 1..]))
}

pub fn record(d: &Diag, root: Option<&str>) -> Value {
    let p = parts(d);
    let mut r = Map::new();
    r.insert("schema".into(), 1.into()); r.insert("type".into(), "diagnostic".into());
    r.insert("code".into(), d.code.into()); r.insert("severity".into(), d.sev.as_str().into());
    r.insert("message".into(), d.message.clone().into());
    r.insert("rustc_code".into(), d.rustc_code.clone().map(Value::from).unwrap_or(Value::Null));
    r.insert("flag".into(), d.flag.clone().map(Value::from).unwrap_or(Value::Null));
    r.insert("package".into(), d.unit.package.clone().into());
    r.insert("target".into(), json!({"name": d.unit.target, "kind": d.unit.kind}));
    r.insert("root".into(), root.map(Value::from).unwrap_or(Value::Null));
    r.insert("files".into(), p.files.into());
    r.insert("symbols".into(), symbols(&d.message).into());
    r.insert("notes".into(), p.notes.into());
    r.insert("suggestions".into(), p.suggestions.into());
    r.insert("locations".into(), p.locations.into());
    r.insert("fixits".into(), p.fixits.into());
    if let Some(t) = type_mismatch(d) { r.insert("type_mismatch".into(), t); }
    r.insert("rendered".into(), d.msg["rendered"].clone());
    Value::Object(r)
}
