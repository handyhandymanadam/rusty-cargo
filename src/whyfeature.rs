//! Feature 9b: why is crate NAME (or one of its features) turned on? Runs `cargo tree -e features -i NAME` with the same package and
//! feature selection as the command line, and turns the inverted tree into a record: every feature of NAME that is on, and the chains of
//! packages and features that switch it on, up to the command line or the root package.
use serde_json::{json, Value};
use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

/// the options that choose which packages, features and platform cargo looks at
pub fn select_args(args: &[OsString]) -> Vec<OsString> {
    let mut out = vec![]; let mut i = 0;
    let with_value = ["--manifest-path", "-p", "--package", "--features", "-F", "--target", "--exclude", "--config", "-C", "-Z", "--profile"];
    let plain = ["--workspace", "--all-features", "--no-default-features", "--locked", "--frozen", "--offline", "--all-targets", "--release", "-r"];
    while i < args.len() {
        let a = args[i].to_string_lossy().into_owned();
        if a == "--" { break; }
        if with_value.contains(&a.as_str()) { out.push(args[i].clone()); if let Some(v) = args.get(i + 1) { out.push(v.clone()); } i += 2; continue; }
        if plain.contains(&a.as_str()) || with_value.iter().any(|w| w.starts_with("--") && a.starts_with(&format!("{w}="))) { out.push(args[i].clone()); }
        i += 1;
    }
    out.retain(|a| a != "--release" && a != "-r" && a != "--all-targets");
    out
}

#[derive(Debug, Clone)]
struct Node { kind: &'static str, name: String, version: Option<String>, feature: Option<String>, note: Option<String>, dup: bool, children: Vec<Node> }

fn parse_line(text: &str) -> Option<Node> {
    let mut t = text.trim().to_string(); let dup = t.ends_with(" (*)"); if dup { t.truncate(t.len() - 4); }
    let note = if t.ends_with(" (command-line)") { t.truncate(t.len() - 15); Some("command-line".to_string()) } else { None };
    let mut it = t.splitn(3, ' ');
    let name = it.next()?.to_string(); let second = it.next()?;
    if second == "feature" {
        let f = it.next()?.trim().trim_matches('"').to_string();
        return Some(Node { kind: "feature", name, version: None, feature: Some(f), note, dup, children: vec![] });
    }
    let version = second.trim_start_matches('v').to_string();
    Some(Node { kind: "package", name, version: Some(version), feature: None, note: it.next().map(|s| s.to_string()).filter(|s| !s.is_empty()), dup, children: vec![] })
}

fn parse_tree(text: &str) -> Vec<Node> {
    let mut roots: Vec<Node> = vec![]; let mut stack: Vec<(usize, Node)> = vec![];
    fn close(stack: &mut Vec<(usize, Node)>, roots: &mut Vec<Node>, to_depth: usize) {
        while stack.last().is_some_and(|(d, _)| *d >= to_depth) {
            let (_, n) = stack.pop().unwrap();
            match stack.last_mut() { Some((_, p)) => p.children.push(n), None => roots.push(n) }
        }
    }
    for line in text.lines() {
        if line.trim().is_empty() { continue; }
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0; while i + 3 < chars.len() && matches!(chars[i], '│' | ' ' | '├' | '└') && !(chars[i] == ' ' && chars[i + 1..].iter().take(3).all(|c| c.is_alphanumeric() || *c == '_')) { i += 4; if chars[i - 4] == '├' || chars[i - 4] == '└' { break; } }
        let depth = i / 4;
        let text: String = chars[i..].iter().collect();
        let Some(n) = parse_line(&text) else { continue };
        close(&mut stack, &mut roots, depth);
        stack.push((depth, n));
    }
    close(&mut stack, &mut roots, 0);
    roots
}

fn label(n: &Node) -> String {
    match n.kind { "feature" => format!("{} feature \"{}\"", n.name, n.feature.as_deref().unwrap_or("")), _ => format!("{} v{}", n.name, n.version.as_deref().unwrap_or("")) }
}

/// every node's label -> the first place its children were printed in full (cargo marks later copies with `(*)` and omits them)
fn defs<'a>(n: &'a Node, m: &mut HashMap<String, &'a Node>) {
    if !n.dup && !n.children.is_empty() { m.entry(label(n)).or_insert(n); }
    for c in &n.children { defs(c, m); }
}

fn paths(n: &Node, defs: &HashMap<String, &Node>, prefix: &mut Vec<String>, out: &mut Vec<Value>) {
    prefix.push(label(n) + &n.note.as_ref().map(|x| if x.starts_with('(') { format!(" {x}") } else { format!(" ({x})") }).unwrap_or_default());
    // a `(*)` copy is followed through the full copy; a loop or a very deep chain stops
    let kids: &[Node] = if n.dup && n.children.is_empty() && prefix.len() < 40 && !prefix[..prefix.len() - 1].contains(prefix.last().unwrap()) { defs.get(&label(n)).map(|d| d.children.as_slice()).unwrap_or(&[]) } else { &n.children };
    if kids.is_empty() { out.push(json!({"chain": prefix})); }
    else { for c in kids { paths(c, defs, prefix, out); } }
    prefix.pop();
}

fn to_json(n: &Node) -> Value {
    json!({"kind": n.kind, "name": n.name, "version": n.version, "feature": n.feature, "note": n.note, "elided_duplicate": n.dup, "children": n.children.iter().map(to_json).collect::<Vec<_>>()})
}

fn collect_features<'a>(n: &'a Node, name: &str, out: &mut Vec<&'a Node>) {
    if n.kind == "feature" && n.name == name && !n.dup { out.push(n); }
    for c in &n.children { collect_features(c, name, out); }
}

pub fn explain(real: &Path, toolchain: Option<&OsString>, select: &[OsString], name: &str) -> Value {
    let mut c = Command::new(real); c.arg0("cargo");
    if let Some(t) = toolchain { c.arg(t); }
    c.args(["tree", "-e", "features", "-i", name]).args(select).env_remove("RUSTY_AGENT").env_remove("RUSTY_AGENT_FD").env("CARGO_TERM_COLOR", "never");
    let mut cmd = vec!["cargo".to_string()]; if let Some(t) = toolchain { cmd.push(t.to_string_lossy().into_owned()); }
    cmd.extend(["tree", "-e", "features", "-i", name].iter().map(|s| s.to_string())); cmd.extend(select.iter().map(|s| s.to_string_lossy().into_owned()));
    let o = match c.output() { Ok(o) => o, Err(e) => return json!({"schema": 1, "type": "why-feature", "query": name, "found": false, "error": e.to_string(), "command": cmd}) };
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    if !o.status.success() || text.trim().is_empty() {
        // not a package: a feature name?
        if let Some(r) = feature_by_name(real, toolchain, select, name, &err) { return r; }
        return json!({"schema": 1, "type": "why-feature", "query": name, "found": false, "exit": o.status.code(), "command": cmd, "error": err.lines().find(|l| l.starts_with("error")).unwrap_or(err.trim()), "rendered": text});
    }
    let roots = parse_tree(&text);
    let mut feats = vec![];
    let mut dmap = HashMap::new(); for r in &roots { defs(r, &mut dmap); }
    for r in &roots {
        let mut found = vec![]; collect_features(r, name.split('@').next().unwrap_or(name), &mut found);
        // the root package line is the crate itself; features of it appear below as the first-level feature nodes
        for f in found {
            let mut ps = vec![]; paths(f, &dmap, &mut vec![], &mut ps);
            feats.push(json!({"feature": f.feature, "enabled_by": ps}));
        }
    }
    let versions: Vec<Value> = roots.iter().filter(|r| r.kind == "package").map(|r| json!({"name": r.name, "version": r.version})).collect();
    fn pkgs(n: &Node, out: &mut Vec<String>) { for c in &n.children { if c.kind == "package" && !out.contains(&label(c)) { out.push(label(c)); } pkgs(c, out); } }
    let mut direct: Vec<String> = vec![]; for r in &roots { pkgs(r, &mut direct); }
    json!({"schema": 1, "type": "why-feature", "query": name, "found": true, "kind": "package", "command": cmd, "packages": versions, "required_by": direct, "features": feats,
           "tree": roots.iter().map(to_json).collect::<Vec<_>>(), "rendered": text})
}

/// `--explain-feature=NAME` where NAME is not a package: look for packages that define a feature called NAME (cargo metadata, offline), and
/// for each ask the package question. `None` when the metadata cannot be read (the caller then reports cargo's own error).
fn feature_by_name(real: &Path, toolchain: Option<&OsString>, select: &[OsString], name: &str, tree_err: &str) -> Option<Value> {
    let mut c = Command::new(real); c.arg0("cargo");
    if let Some(t) = toolchain { c.arg(t); }
    c.args(["metadata", "--format-version=1", "--offline"]);
    // metadata takes the manifest and the feature selection, not -p / --target / --workspace
    let mut i = 0;
    while i < select.len() {
        let a = select[i].to_string_lossy();
        let value = matches!(&*a, "--manifest-path" | "--features" | "-F");
        let plain = matches!(&*a, "--all-features" | "--no-default-features" | "--locked" | "--frozen") || a.starts_with("--features=") || a.starts_with("--manifest-path=");
        if value { c.arg(&select[i]); if let Some(v) = select.get(i + 1) { c.arg(v); } i += 2; continue; }
        if plain { c.arg(&select[i]); }
        i += 1;
    }
    c.env_remove("RUSTY_AGENT").env_remove("RUSTY_AGENT_FD").env("CARGO_TERM_COLOR", "never");
    let o = c.output().ok().filter(|o| o.status.success())?;
    let meta: Value = serde_json::from_slice(&o.stdout).ok()?;
    let pkgs = meta["packages"].as_array()?;
    let mut owners: Vec<(String, String)> = vec![];   // packages that define a feature called NAME
    let (mut pkg_names, mut feat_names): (Vec<String>, Vec<String>) = (vec![], vec![]);
    for p in pkgs {
        let (pn, pv) = (p["name"].as_str().unwrap_or("").to_string(), p["version"].as_str().unwrap_or("").to_string());
        if let Some(f) = p["features"].as_object() {
            if f.contains_key(name) { owners.push((pn.clone(), pv.clone())); }
            for k in f.keys() { if !feat_names.contains(k) { feat_names.push(k.clone()); } }
        }
        if !pkg_names.contains(&pn) { pkg_names.push(pn); }
    }
    if owners.is_empty() {
        let mut close: Vec<(usize, String)> = pkg_names.iter().chain(feat_names.iter()).filter(|n| n.as_str() != name)
            .map(|n| (crate::explain::edit_distance(name, n), n.clone())).filter(|(d, _)| *d <= 2).collect();
        close.sort(); close.dedup_by(|a, b| a.1 == b.1);
        let close: Vec<String> = close.into_iter().take(5).map(|x| x.1).collect();
        return Some(json!({"schema": 1, "type": "why-feature", "query": name, "found": false,
            "hint": "NAME must be a package in the dependency graph or a feature of one of its packages; neither matched", "close_names": close,
            "error": tree_err.lines().find(|l| l.starts_with("error")).unwrap_or("").to_string()}));
    }
    let (mut on, mut off): (Vec<Value>, Vec<String>) = (vec![], vec![]);
    let total = owners.len();
    for (pn, pv) in owners.iter().take(12) {
        let spec = format!("{pn}@{pv}");
        let r = explain(real, toolchain, select, &spec);
        let chains: Vec<Value> = r["features"].as_array().into_iter().flatten().filter(|f| f["feature"] == name).flat_map(|f| f["enabled_by"].as_array().cloned().unwrap_or_default()).collect();
        if chains.is_empty() { off.push(format!("{pn} v{pv}")); } else { on.push(json!({"package": format!("{pn} v{pv}"), "enabled_by": chains})); }
    }
    let mut rec = json!({"schema": 1, "type": "why-feature", "query": name, "found": true, "kind": "feature", "enabled": !on.is_empty(), "packages": on, "defined_but_off": off});
    if total > 12 { rec["packages_omitted"] = (total - 12).into(); }
    Some(rec)
}
