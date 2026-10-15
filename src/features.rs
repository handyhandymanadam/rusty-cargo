//! Hooks the driver calls for the features that read more than diagnostics (tests, panics, fix preview, ...).
use crate::config::Config;
use crate::diag::Diag;
use crate::driver::{Out, State};
use crate::explain::Explainer;
use crate::fixpreview::FixPreview;
use crate::testreport::TestReport;
use serde_json::Value;

pub struct Ctx { emit_tests: bool, panic_on: bool, pub panics: Option<crate::panicreport::Scanner>, pub locals: crate::panicreport::Locals, pub fix: FixPreview, fix_on: bool, pub explain: Option<Explainer>, pub tests: Option<TestReport>, stable: bool, exes: Vec<(String, String, String)>, pub test_totals: Option<Value>, pub why: crate::whyrebuilt::WhyRebuilt, real: std::path::PathBuf, toolchain: Option<std::ffi::OsString>, select: Vec<std::ffi::OsString>, feature_queries: Vec<String> }

impl Ctx {
    pub fn new(cfg: &Config, real: &std::path::Path, toolchain: Option<std::ffi::OsString>, args: &[std::ffi::OsString]) -> Ctx {
        Ctx { panics: cfg.panic_report.then(Default::default), locals: Default::default(), why: crate::whyrebuilt::WhyRebuilt::new(cfg.why_rebuilt.clone()), real: real.to_path_buf(), select: crate::whyfeature::select_args(args), feature_queries: cfg.why_feature.clone(), toolchain: toolchain.clone(), fix: FixPreview::default(), fix_on: cfg.fix_preview, explain: cfg.missing_crate.then(|| Explainer::new(real, toolchain)),
               tests: (cfg.test_report || cfg.panic_report).then(TestReport::default), emit_tests: cfg.test_report, panic_on: cfg.panic_report, stable: cfg.stable, exes: vec![], test_totals: None }
    }
    /// extra records that go right after a diagnostic's own record
    pub fn extra_records(&mut self, d: &Diag) -> Vec<Value> { self.explain.as_mut().and_then(|e| e.explain(d)).into_iter().collect() }
    /// every diagnostic record that is written passes through here
    pub fn diag_record(&mut self, rec: &Value) { if self.fix_on { self.fix.add(rec); } }
    pub fn artifact(&mut self, v: &Value) {
        if self.panics.is_some() || self.tests.is_some() { self.locals.add(v["package_id"].as_str().unwrap_or(""), v["manifest_path"].as_str().unwrap_or(""), v["target"]["name"].as_str().unwrap_or("")); }
        if let Some(exe) = v["executable"].as_str() {
            let u = crate::diag::Unit::from_msg(v);
            self.exes.push((exe.to_string(), u.package, u.target));
        }
    }
    pub fn stdout_line(&mut self, line: &[u8], _out: &mut Out) { if let Some(t) = &mut self.tests { t.line(line); } }
    pub fn stderr_line(&mut self, line: &[u8], _out: &mut Out) { if let Some(p) = &mut self.panics { p.line(line, &self.locals); } }
    /// true = the line is ours (consumed)
    pub fn status(&mut self, verb: &str, rest: &str) -> bool {
        let consumed = self.why.active() && self.why.status(verb, rest);
        if let Some(t) = &mut self.tests {
            match verb { "Running" => t.running_binary(rest), "Doc-tests" => t.doc_tests(rest), _ => {} }
        }
        consumed
    }
    /// a CARGO_LOG line; true = consumed
    pub fn log_line(&mut self, line: &[u8]) -> bool { self.why.active() && self.why.log_line(&String::from_utf8_lossy(line)) }
    pub fn feature_records(&self) -> Vec<Value> {
        self.feature_queries.iter().map(|q| crate::whyfeature::explain(&self.real, self.toolchain.as_ref(), &self.select, q)).collect()
    }
    pub fn finish(&mut self, out: &mut Out, _st: &State) {
        if let Some(p) = &mut self.panics { for r in p.take(&self.locals) { out.rec(&r); } }
        if let Some(t) = &mut self.tests {
            let exes = &self.exes;
            let unit_of = |path: &str| exes.iter().find(|(e, _, _)| !path.is_empty() && (e == path || e.ends_with(path))).map(|(_, p, t)| (p.clone(), t.clone()));
            let (recs, totals) = t.records(self.stable, &unit_of, self.panic_on.then_some(&self.locals), self.emit_tests);
            for r in &recs { out.rec(r); }
            if t.seen && self.emit_tests { self.test_totals = Some(totals); }
        }
        if self.fix_on { out.rec(&self.fix.record()); }
        if self.why.active() { for r in self.why.records() { out.rec(&r); } }
        for r in self.feature_records() { out.rec(&r); }
    }
    pub fn add_summary(&self, r: &mut Value) { if let Some(t) = &self.test_totals { r["tests"] = t.clone(); } }
}

/// extra fields on a diagnostic record from the opt-in features
pub fn decorate(cfg: &Config, d: &Diag, rec: &mut Value) {
    if cfg.ownership { if let Some(t) = crate::ownership::trace(d) { rec["ownership_trace"] = t; } }
}
