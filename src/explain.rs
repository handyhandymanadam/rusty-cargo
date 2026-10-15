//! Feature 6: for "unresolved import" and "unlinked crate" errors, say which crate is meant, the Cargo.toml line, and, when the
//! item is behind a feature, which feature. Offline: it reads `cargo metadata --offline`, Cargo.lock's graph through it, and the
//! crates already in the local registry cache. It never starts a download.
use crate::diag::Diag;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::os::unix::process::CommandExt;
use std::process::Command;

pub struct Explainer {
    real: PathBuf, toolchain: Option<OsString>,
    meta: Vec<(String, Option<Value>)>,   // manifest -> metadata (None = could not be read)
    cached: Option<Vec<(String, String)>>, // (crate, version) from the registry cache
    done: Vec<String>,
}

impl Explainer {
    pub fn new(real: &Path, toolchain: Option<OsString>) -> Explainer { Explainer { real: real.to_path_buf(), toolchain, meta: vec![], cached: None, done: vec![] } }

    fn metadata(&mut self, manifest: &str) -> Option<&Value> {
        if !self.meta.iter().any(|(m, _)| m == manifest) {
            let mut c = Command::new(&self.real); c.arg0("cargo");
            if let Some(t) = &self.toolchain { c.arg(t); }
            c.args(["metadata", "--format-version=1", "--offline", "--manifest-path", manifest]).env_remove("RUSTY_AGENT").env_remove("RUSTY_AGENT_FD");
            let v = c.output().ok().filter(|o| o.status.success()).and_then(|o| serde_json::from_slice::<Value>(&o.stdout).ok());
            self.meta.push((manifest.to_string(), v));
        }
        self.meta.iter().find(|(m, _)| m == manifest).and_then(|(_, v)| v.as_ref())
    }

    fn cached(&mut self) -> &Vec<(String, String)> {
        self.cached.get_or_insert_with(|| {
            let mut out = vec![];
            let home = std::env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")));
            if let Some(h) = home {
                if let Ok(rd) = std::fs::read_dir(h.join("registry/cache")) {
                    for idx in rd.flatten() {
                        if let Ok(files) = std::fs::read_dir(idx.path()) {
                            for f in files.flatten() {
                                let n = f.file_name().to_string_lossy().into_owned();
                                if let Some(stem) = n.strip_suffix(".crate") { if let Some((name, ver)) = split_name_version(stem) { out.push((name, ver)); } }
                            }
                        }
                    }
                }
            }
            out.sort(); out.dedup(); out
        })
    }

    pub fn explain(&mut self, d: &Diag) -> Option<Value> {
        if !matches!(d.rustc_code.as_deref(), Some("E0432" | "E0433" | "E0463")) || d.msg.is_null() { return None; }
        let primary = crate::diag::primary_span(&d.msg)?;
        let label = primary["label"].as_str().unwrap_or("");
        // 1. an item behind a feature: rustc names the feature itself in a note label
        let gated = d.msg["children"].as_array().into_iter().flatten().flat_map(|c| c["spans"].as_array().into_iter().flatten()).find_map(|s| {
            let l = s["label"].as_str()?; let a = l.find("gated behind the `")?; let r = &l[a + 18..]; let f = &r[..r.find('`')?];
            Some((f.to_string(), s["file_name"].as_str()?.to_string(), l.to_string()))
        });
        let path_text = first_backtick(&d.message).unwrap_or_default();
        let loc = json!({"file": primary["file_name"], "line": primary["line_start"], "column": primary["column_start"]});
        let section = match d.unit.kind.as_str() { "test" | "example" | "bench" => "dev-dependencies", "custom-build" => "build-dependencies", _ => "dependencies" };
        let manifest = d.unit.manifest.clone();
        if let Some((feature, file, evidence)) = gated {
            let (dep, version) = registry_crate_of(&file)?;
            let key = format!("{manifest}|{dep}|{feature}"); if self.done.contains(&key) { return None; } self.done.push(key);
            let req = self.metadata(&manifest).and_then(|m| dep_req(m, &manifest, &dep)).unwrap_or_else(|| format!("{}", version));
            let line = format!("{dep} = {{ version = \"{req}\", features = [\"{feature}\"] }}");
            return Some(json!({"schema": 1, "type": "missing-crate", "code": "R0003", "rustc_code": d.rustc_code, "situation": "feature-gated", "name": dep, "path": path_text,
                "location": loc, "package": d.unit.package, "manifest": manifest, "section": section, "found": {"in_manifest": true, "locked_version": version, "cached_versions": [version]},
                "features": [{"name": feature, "evidence": evidence, "evidence_file": file}], "cargo_toml_line": line, "add_command": format!("cargo add {dep} --features {feature} --offline"),
                "network": "not used", "suggestions": [{"message": format!("enable feature `{feature}` of `{dep}` in {section}"), "option": format!("cargo add {dep} --features {feature} --offline")}]}));
        }
        // 2. a crate that is not linked
        let name = if let Some(a) = label.find("unlinked crate `") { let r = &label[a + 16..]; r[..r.find('`')?].to_string() } else if !path_text.contains("::") && !path_text.is_empty() { path_text.clone() } else { return None };
        if matches!(name.as_str(), "std" | "core" | "alloc" | "crate" | "self" | "super") { return None; }
        let key = format!("{manifest}|{name}|crate|{section}"); if self.done.contains(&key) { return None; } self.done.push(key);
        let norm = |s: &str| s.replace('-', "_");
        let mut found = json!({"in_manifest": false, "optional": false, "locked_version": null, "cached_versions": [], "workspace_member": false, "path": null});
        let mut situation = "not-a-dependency"; let mut line: Option<String> = None; let mut add: Option<String> = None; let mut sugg: Vec<Value> = vec![];
        let meta = self.metadata(&manifest).cloned();
        let mut pkg_dir = Path::new(&manifest).parent().map(|p| p.to_path_buf());
        if let Some(m) = &meta {
            let pkg = m["packages"].as_array().into_iter().flatten().find(|p| p["manifest_path"] == manifest.as_str());
            if let Some(p) = pkg {
                pkg_dir = Path::new(p["manifest_path"].as_str().unwrap_or("")).parent().map(|x| x.to_path_buf());
                if let Some(dep) = p["dependencies"].as_array().into_iter().flatten().find(|x| norm(x["rename"].as_str().unwrap_or(x["name"].as_str().unwrap_or(""))) == name) {
                    let kind = dep["kind"].as_str().unwrap_or("normal");
                    found["in_manifest"] = true.into(); found["optional"] = dep["optional"].clone();
                    let dname = dep["name"].as_str().unwrap_or(&name).to_string();
                    if dep["optional"] == true {
                        situation = "optional-dependency-off";
                        let feats: Vec<String> = p["features"].as_object().into_iter().flatten().filter(|(_, v)| v.as_array().is_some_and(|a| a.iter().any(|x| x.as_str() == Some(&format!("dep:{dname}")) || x.as_str() == Some(dname.as_str())))).map(|(k, _)| k.clone()).collect();
                        let f0 = feats.first().cloned().unwrap_or(dname.clone());
                        add = Some(format!("cargo build --features {f0}")); line = None;
                        sugg.push(json!({"message": format!("`{dname}` is an optional dependency; enable the feature `{f0}`"), "option": format!("--features {f0}")}));
                    } else if kind == "dev" && section != "dev-dependencies" || kind == "build" && section != "build-dependencies" {
                        situation = "wrong-dependency-kind";
                        line = Some(format!("{dname} = \"{}\"", dep["req"].as_str().unwrap_or("*").trim_start_matches('^')));
                        sugg.push(json!({"message": format!("`{dname}` is declared as a {kind} dependency but this target needs it in [{section}]"), "option": null}));
                    } else { situation = "declared-but-unlinked"; }
                }
            }
            if situation == "not-a-dependency" {
                let members: BTreeSet<&str> = m["workspace_members"].as_array().into_iter().flatten().filter_map(|x| x.as_str()).collect();
                let cand = m["packages"].as_array().into_iter().flatten().filter(|p| norm(p["name"].as_str().unwrap_or("")) == name).collect::<Vec<_>>();
                if let Some(w) = cand.iter().find(|p| p["source"].is_null() && members.contains(p["id"].as_str().unwrap_or(""))) {
                    let dir = Path::new(w["manifest_path"].as_str().unwrap_or("")).parent().unwrap_or(Path::new("")).to_path_buf();
                    let rel = relative(pkg_dir.as_deref().unwrap_or(Path::new("/")), &dir);
                    found["workspace_member"] = true.into(); found["path"] = rel.clone().into(); situation = "workspace-member";
                    let n = w["name"].as_str().unwrap_or(&name);
                    line = Some(format!("{n} = {{ path = \"{rel}\" }}")); add = Some(format!("cargo add --path {rel}"));
                    sugg.push(json!({"message": format!("`{n}` is a member of this workspace; add it as a path dependency"), "option": add}));
                } else if let Some(l) = cand.iter().max_by_key(|p| ver_key(p["version"].as_str().unwrap_or(""))) {
                    let v = l["version"].as_str().unwrap_or("").to_string(); let n = l["name"].as_str().unwrap_or(&name).to_string();
                    found["locked_version"] = v.clone().into(); situation = "in-lockfile-not-a-dependency";
                    line = Some(format!("{n} = \"{v}\"")); add = Some(format!("cargo add {n}@{v} --offline"));
                    sugg.push(json!({"message": format!("`{n}` {v} is already in Cargo.lock (pulled in by another dependency); declaring it needs no download"), "option": add}));
                }
            }
        }
        if situation == "not-a-dependency" || situation == "in-lockfile-not-a-dependency" {
            let hyph = name.replace('_', "-");
            let vers: Vec<(String, String)> = self.cached().iter().filter(|(n, _)| *n == name || *n == hyph).cloned().collect();
            if !vers.is_empty() {
                let n = vers[0].0.clone();
                let mut vs: Vec<String> = vers.iter().map(|x| x.1.clone()).collect(); vs.sort_by_key(|v| ver_key(v)); vs.dedup();
                found["cached_versions"] = vs.clone().into();
                if situation == "not-a-dependency" {
                    let best = vs.last().cloned().unwrap_or_default(); situation = "in-registry-cache";
                    line = Some(format!("{n} = \"{best}\"")); add = Some(format!("cargo add {n}@{best} --offline"));
                    sugg.push(json!({"message": format!("`{n}` is in the local registry cache (versions {}); it can be added without a download", vs.join(", ")), "option": add}));
                }
            }
        }
        let mut near: Vec<Value> = vec![];
        if situation == "not-a-dependency" {
            let mut names: BTreeSet<String> = self.cached().iter().map(|x| x.0.clone()).collect();
            if let Some(m) = &meta { for p in m["packages"].as_array().into_iter().flatten() { if let Some(n) = p["name"].as_str() { names.insert(n.to_string()); } } }
            let mut c: Vec<(usize, String)> = names.into_iter().filter(|n| n.len() >= 3).map(|n| (edit_distance(&norm(&n), &name), n)).filter(|(d, _)| *d <= 2).collect();
            c.sort(); c.truncate(3);
            near = c.into_iter().map(|(d, n)| json!({"name": n, "distance": d})).collect();
            let a = format!("cargo add {name}");
            line = Some(format!("{name} = \"<version>\""));
            sugg.push(json!({"message": format!("`{name}` is not in Cargo.toml, Cargo.lock or the local registry cache; adding it needs your cargo's own network access"), "option": a}));
            add = Some(a);
        }
        Some(json!({"schema": 1, "type": "missing-crate", "code": "R0003", "rustc_code": d.rustc_code, "situation": situation, "name": name, "path": path_text, "location": loc,
            "package": d.unit.package, "manifest": manifest, "section": section, "found": found, "features": [], "cargo_toml_line": line, "add_command": add,
            "near_misses": near, "network": "not used", "suggestions": sugg}))
    }
}

fn first_backtick(s: &str) -> Option<String> { let a = s.find('`')?; let r = &s[a + 1..]; Some(r[..r.find('`')?].to_string()) }

/// "/srv/u/.cargo/registry/src/index.crates.io-xxxx/serde_json-1.0.154/src/value/mod.rs" -> ("serde_json", "1.0.154")
fn registry_crate_of(file: &str) -> Option<(String, String)> {
    let a = file.find("/registry/src/")?; let rest = &file[a + 14..];
    let mut it = rest.split('/'); it.next()?; split_name_version(it.next()?)
}

fn split_name_version(s: &str) -> Option<(String, String)> {
    let b = s.as_bytes();
    for i in (1..b.len()).rev() { if b[i - 1] == b'-' && b[i].is_ascii_digit() { return Some((s[..i - 1].to_string(), s[i..].to_string())); } }
    None
}

fn dep_req(m: &Value, manifest: &str, dep: &str) -> Option<String> {
    let p = m["packages"].as_array()?.iter().find(|p| p["manifest_path"] == manifest)?;
    let d = p["dependencies"].as_array()?.iter().find(|d| d["name"] == dep)?;
    Some(d["req"].as_str()?.trim_start_matches('^').to_string())
}

/// sortable key for "1.2.3-pre+meta": numbers first; a pre-release sorts below its release
fn ver_key(v: &str) -> (Vec<u64>, bool) {
    let core = v.split(['-', '+']).next().unwrap_or("");
    (core.split('.').map(|x| x.parse().unwrap_or(0)).collect(), !v.contains('-'))
}

fn relative(from: &Path, to: &Path) -> String {
    let f: Vec<_> = from.components().collect(); let t: Vec<_> = to.components().collect();
    let n = f.iter().zip(&t).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".into(); f.len() - n];
    parts.extend(t[n..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    if parts.is_empty() { ".".into() } else { parts.join("/") }
}

pub(crate) fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i];
        for j in 1..=b.len() { cur.push((prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1]))); }
        prev = cur;
    }
    prev[b.len()]
}
