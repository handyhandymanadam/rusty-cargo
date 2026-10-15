//! Which directory a diagnostic's relative file names are relative to (cargo runs rustc from the workspace root).
use crate::diag::Diag;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

thread_local! { static CACHE: RefCell<HashMap<(String, String), Option<String>>> = RefCell::new(HashMap::new()); }

fn file_of(d: &Diag) -> Option<String> {
    if let Some(p) = &d.primary { return Some(p.file.clone()); }
    d.msg["spans"].as_array()?.iter().find_map(|s| s["file_name"].as_str().map(|x| x.to_string()))
}

pub fn root_for(d: &Diag) -> Option<String> {
    let file = file_of(d)?;
    if file.starts_with('/') { return None; }
    let key = (d.unit.manifest.clone(), file.clone());
    if let Some(v) = CACHE.with(|c| c.borrow().get(&key).cloned()) { return v; }
    let mut cands: Vec<PathBuf> = vec![];
    if let Some(dir) = Path::new(&d.unit.manifest).parent() { for a in dir.ancestors() { if a != Path::new("") && a != Path::new("/") { cands.push(a.to_path_buf()); } } }
    if let Ok(cwd) = std::env::current_dir() { cands.push(cwd); }
    let found = cands.into_iter().find(|c| c.join(&file).exists()).map(|c| c.to_string_lossy().into_owned());
    CACHE.with(|c| c.borrow_mut().insert(key, found.clone()));
    found
}
