//! Agent-mode switches: RUSTY_AGENT=1 or --agent-mode, plus one flag per feature. Our flags are removed
//! from the argument list before cargo sees it. Nothing after a bare `--` is looked at (those arguments belong to
//! rustc, the test harness or the program being run).
use std::ffi::OsString;

#[derive(Default, Debug, Clone)]
pub struct Config {
    pub agent: bool,
    pub diag_json: bool,
    pub root_cause: bool,
    pub build_summary: bool,
    pub stable: bool,
    pub test_report: bool,
    pub ownership: bool,
    pub fix_preview: bool,
    pub missing_crate: bool,
    pub why_rebuilt: Vec<String>,
    pub why_feature: Vec<String>,
    pub panic_report: bool,
    pub agent_help: bool,
    pub detail: crate::shape::Detail,
}

pub struct Parsed { pub cfg: Config, pub args: Vec<OsString> }

/// The flags we own. Anything else is cargo's.
pub const OUR_FLAGS: &[&str] = &[
    "--agent-mode", "--no-agent-mode", "--agent-help", "--diagnostics-format=json", "--diagnostics-format=text",
    "--root-cause", "--no-root-cause", "--build-summary", "--no-build-summary", "--stable-diagnostics", "--no-stable-diagnostics",
    "--test-report", "--no-test-report", "--ownership-trace", "--fix-preview", "--explain-missing-crate", "--panic-report",
];
pub const OUR_PREFIXES: &[&str] = &["--explain-rebuild=", "--explain-feature=", "--agent-detail="];

pub fn is_ours(a: &str) -> bool { OUR_FLAGS.contains(&a) || OUR_PREFIXES.iter().any(|p| a.starts_with(p)) }

pub fn parse(args: Vec<OsString>) -> Parsed {
    let env_on = std::env::var_os("RUSTY_AGENT").is_some_and(|v| v == "1");
    let mut cfg = Config { agent: env_on, ..Default::default() };
    // RUSTY_AGENT_DETAIL=brief|normal|full; the flag --agent-detail=... wins; an unknown value changes nothing
    if let Some(d) = std::env::var("RUSTY_AGENT_DETAIL").ok().and_then(|v| crate::shape::Detail::parse(&v)) { cfg.detail = d; }
    let (mut diag_text, mut no_root, mut no_summary, mut no_stable, mut no_test) = (false, false, false, false, false);
    let mut kept = Vec::with_capacity(args.len());
    let mut after_dd = false;
    for a in args {
        if !after_dd {
            if let Some(t) = a.to_str() {
                if t == "--" { after_dd = true; }
                else if is_ours(t) {
                    match t {
                        "--agent-mode" => cfg.agent = true,
                        "--no-agent-mode" => cfg.agent = false,
                        "--agent-help" => cfg.agent_help = true,
                        "--diagnostics-format=json" => cfg.diag_json = true,
                        "--diagnostics-format=text" => diag_text = true,
                        "--root-cause" => cfg.root_cause = true,
                        "--no-root-cause" => no_root = true,
                        "--build-summary" => cfg.build_summary = true,
                        "--no-build-summary" => no_summary = true,
                        "--stable-diagnostics" => cfg.stable = true,
                        "--no-stable-diagnostics" => no_stable = true,
                        "--test-report" => cfg.test_report = true,
                        "--no-test-report" => no_test = true,
                        "--ownership-trace" => cfg.ownership = true,
                        "--fix-preview" => cfg.fix_preview = true,
                        "--explain-missing-crate" => cfg.missing_crate = true,
                        "--panic-report" => cfg.panic_report = true,
                        _ => {
                            if let Some(v) = t.strip_prefix("--explain-rebuild=") { cfg.why_rebuilt.push(v.to_string()); }
                            else if let Some(d) = t.strip_prefix("--agent-detail=").and_then(crate::shape::Detail::parse) { cfg.detail = d; }
                            else if let Some(v) = t.strip_prefix("--explain-feature=") { cfg.why_feature.push(v.to_string()); }
                        }
                    }
                    continue;
                }
            }
        }
        kept.push(a);
    }
    // The umbrella: the useful records together. Heavier or occasional ones stay separate flags.
    if cfg.agent {
        if !diag_text { cfg.diag_json = true; }
        if !no_root { cfg.root_cause = true; }
        if !no_summary { cfg.build_summary = true; }
        if !no_stable { cfg.stable = true; }
        if !no_test { cfg.test_report = true; }
    }
    if no_root { cfg.root_cause = false; }
    if no_summary { cfg.build_summary = false; }
    if no_stable { cfg.stable = false; }
    if no_test { cfg.test_report = false; }
    // features that read diagnostics need the diagnostic stream
    if cfg.root_cause || cfg.ownership || cfg.fix_preview || cfg.missing_crate { cfg.diag_json = true; }
    if diag_text && !cfg.root_cause && !cfg.ownership && !cfg.fix_preview && !cfg.missing_crate { cfg.diag_json = false; }
    Parsed { cfg, args: kept }
}

impl Config {
    /// true when the wrapper must stay alive and read cargo's output
    pub fn needs_driver(&self) -> bool {
        self.agent || self.diag_json || self.build_summary || self.test_report || self.fix_preview || self.stable
            || self.panic_report || !self.why_rebuilt.is_empty() || !self.why_feature.is_empty()
    }
}
