//! Feature 5: a dry run of `cargo fix`. MachineApplicable suggestions are applied in memory and shown as unified diffs.
//! Nothing is written. One round only (cargo fix repeats rounds until nothing changes). A suggestion is all-or-nothing:
//! its parts are applied together or not at all; overlapping suggestions are reported as conflicts and skipped.
use serde_json::{json, Value};
use similar::TextDiff;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
struct Edit { file: String, root: Option<String>, start: usize, end: usize, text: String }

#[derive(Clone, Debug)]
struct Group { edits: Vec<Edit>, safe: bool, applicability: String, message: String, code: String, flag: Option<String> }

#[derive(Default)]
pub struct FixPreview { groups: Vec<Group> }

impl FixPreview {
    /// takes the finished diagnostic record, so what is previewed is exactly what the record's fixits say
    pub fn add(&mut self, rec: &Value) {
        let root = rec["root"].as_str().map(|s| s.to_string());
        let (Some(sugg), Some(fixits)) = (rec["suggestions"].as_array(), rec["fixits"].as_array()) else { return };
        for s in sugg {
            let mut edits = vec![];
            for i in s["fixits"].as_array().into_iter().flatten() {
                let Some(f) = i.as_u64().and_then(|i| fixits.get(i as usize)) else { continue };
                let (Some(file), Some(a), Some(b)) = (f["file"].as_str(), f["start"]["offset"].as_u64(), f["end"]["offset"].as_u64()) else { continue };
                edits.push(Edit { file: file.to_string(), root: root.clone(), start: a as usize, end: b as usize, text: f["text"].as_str().unwrap_or("").to_string() });
            }
            if edits.is_empty() { continue; }
            let applicability = s["applicability"].as_str().unwrap_or("Unspecified").to_string();
            let g = Group { edits, safe: applicability == "MachineApplicable", applicability, message: s["message"].as_str().unwrap_or("").to_string(),
                            code: rec["code"].as_str().unwrap_or("").to_string(), flag: rec["flag"].as_str().map(|x| x.to_string()).or_else(|| rec["rustc_code"].as_str().map(|x| x.to_string())) };
            // the same suggestion is reported once per target that has the code (lib and test builds): keep one
            let key = |g: &Group| g.edits.iter().map(|e| (e.file.clone(), e.start, e.end, e.text.clone())).collect::<Vec<_>>();
            if !self.groups.iter().any(|x| key(x) == key(&g)) { self.groups.push(g); }
        }
    }

    pub fn record(&self) -> Value {
        let mut by_file: BTreeMap<(String, Option<String>), Vec<&Group>> = BTreeMap::new();
        for g in &self.groups {
            // a group lives with its first file; a multi-file group is rare and kept whole
            let e = &g.edits[0]; by_file.entry((e.file.clone(), e.root.clone())).or_default().push(g);
        }
        let mut files = vec![]; let mut all_edits = vec![];
        for ((file, root), groups) in by_file {
            let path = match &root { Some(r) => format!("{r}/{file}"), None => file.clone() };
            let Ok(bytes) = std::fs::read(&path) else {
                files.push(json!({"file": file, "root": root, "error": "cannot read the file", "safe_edits": 0, "unsafe_edits": 0, "conflicts": 0, "diff": "", "diff_with_unsafe": ""})); continue;
            };
            let Ok(orig) = String::from_utf8(bytes) else { files.push(json!({"file": file, "root": root, "error": "file is not UTF-8", "safe_edits": 0, "unsafe_edits": 0, "conflicts": 0, "diff": "", "diff_with_unsafe": ""})); continue };
            let (safe_text, safe_res) = apply(&orig, &groups, true);
            let (all_text, all_res) = apply(&orig, &groups, false);
            let hdr = |s: &str| format!("{s}/{file}");
            let diff = |new: &str| if new == orig { String::new() } else { TextDiff::from_lines(orig.as_str(), new).unified_diff().context_radius(3).header(&hdr("a"), &hdr("b")).to_string() };
            let mut conflicts = 0;
            for (gi, g) in groups.iter().enumerate() {
                let in_safe = safe_res.get(&gi).copied().unwrap_or(false); let in_all = all_res.get(&gi).copied().unwrap_or(false);
                if (g.safe && !in_safe) || (!g.safe && !in_all) { conflicts += 1; }
                for e in &g.edits {
                    all_edits.push(json!({"file": e.file, "start_offset": e.start, "end_offset": e.end, "text": e.text, "safety": if g.safe { "safe" } else { "unsafe" },
                        "applicability": g.applicability, "message": g.message, "code": g.code, "from": g.flag, "in_diff": g.safe && in_safe, "in_diff_with_unsafe": in_all}));
                }
            }
            files.push(json!({"file": file, "root": root, "safe_edits": groups.iter().enumerate().filter(|(i, g)| g.safe && safe_res.get(i) == Some(&true)).count(),
                "unsafe_edits": groups.iter().enumerate().filter(|(i, g)| !g.safe && all_res.get(i) == Some(&true)).count(), "conflicts": conflicts,
                "diff": diff(&safe_text), "diff_with_unsafe": diff(&all_text)}));
        }
        json!({"schema": 1, "type": "fix-preview", "dry_run": true, "written": false, "rounds": 1, "files": files, "edits": all_edits,
               "apply": "patch -p1 from each file's root directory (safe diff); cargo fix repeats rounds, this is the first"})
    }
}

/// two edits conflict when their ranges overlap, or when both insert at the same place
fn overlaps(x: (usize, usize), y: (usize, usize)) -> bool {
    if x.0 == x.1 && y.0 == y.1 { return x.0 == y.0; }
    x.0 < y.1 && y.0 < x.1
}

/// applies groups (only the safe ones when `safe_only`) sorted by position; returns the new text and which group indexes went in
fn apply(orig: &str, groups: &[&Group], safe_only: bool) -> (String, BTreeMap<usize, bool>) {
    let mut order: Vec<usize> = (0..groups.len()).filter(|i| !safe_only || groups[*i].safe).collect();
    // safe suggestions go first so an unsafe one never blocks a safe one; then by position
    order.sort_by_key(|i| (!groups[*i].safe, groups[*i].edits.iter().map(|e| e.start).min().unwrap_or(0)));
    let mut taken: Vec<(usize, usize)> = vec![]; let mut accepted: Vec<usize> = vec![]; let mut res = BTreeMap::new();
    for gi in order {
        let g = groups[gi];
        let ok = g.edits.iter().all(|e| e.start <= e.end && e.end <= orig.len() && orig.is_char_boundary(e.start) && orig.is_char_boundary(e.end)
            && !taken.iter().any(|&(a, b)| overlaps((e.start, e.end), (a, b))));
        // two parts of one group may touch each other; they are checked against earlier groups only
        res.insert(gi, ok);
        if ok { for e in &g.edits { taken.push((e.start, e.end)); } accepted.push(gi); }
    }
    let mut edits: Vec<&Edit> = accepted.iter().flat_map(|gi| groups[*gi].edits.iter()).collect();
    edits.sort_by_key(|e| (e.start, e.end));
    let mut out = String::with_capacity(orig.len()); let mut pos = 0;
    for e in edits { if e.start < pos { continue; } out.push_str(&orig[pos..e.start]); out.push_str(&e.text); pos = e.end; }
    out.push_str(&orig[pos..]);
    (out, res)
}
