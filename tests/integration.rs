//! Runs the python test scripts (real cargo jobs, JSON parsed in python) against the release-profile binary under test.
use std::process::Command;
use std::sync::Mutex;

/// one build at a time (project rule): the scripts take turns even though the harness runs tests in parallel
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn script(name: &str) {
    let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let root = env!("CARGO_MANIFEST_DIR");
    let st = Command::new("python3").arg("-I").arg(format!("{root}/tests/{name}")).env("RUSTY_CARGO_BIN", env!("CARGO_BIN_EXE_rusty-cargo")).status().expect("python3");
    assert!(st.success(), "{name} failed");
}

#[test] fn passthrough() { script("passthrough.py"); }
#[test] fn diagnostics() { script("agent_diag.py"); }
#[test] fn ownership() { script("agent_ownership.py"); }
#[test] fn fix_preview() { script("agent_fixpreview.py"); }
#[test] fn missing_crate() { script("agent_missingcrate.py"); }
#[test] fn test_report() { script("agent_tests.py"); }
#[test] fn why_rebuilt_and_feature() { script("agent_why.py"); }
#[test] fn panic_report() { script("agent_panic.py"); }
#[test] fn help_and_collisions() { script("agent_help.py"); }
#[test] fn followup_records_file_and_closing_lines() { script("agent_followup.py"); }
#[test] fn agent_fd() { script("agent_fd.py"); }
#[test] fn agent_fixes_round() { script("agent_fixes.py"); }
#[test] fn detail_levels_noise_and_size_budget() { script("agent_detail.py"); }
