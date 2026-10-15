//! Self-description (feature 14): the section appended to `--help` in agent mode, and `--agent-help` as one JSON object.
use serde_json::{json, Value};

/// (name, in the --agent-mode umbrella, applies to, description)
const OPTIONS: &[(&str, bool, &str, &str)] = &[
    ("--agent-mode", false, "any", "turn agent mode on (same as RUSTY_AGENT=1); removed before cargo sees the arguments"),
    ("--no-agent-mode", false, "any", "turn it off again, also over RUSTY_AGENT=1"),
    ("--diagnostics-format=json", true, "build check clippy test bench run doc rustc rustdoc", "every compiler and clippy message as one `diagnostic` record; `=text` gives cargo's own text back"),
    ("--root-cause", true, "same", "group follow-on errors under the first cause (`--no-root-cause` writes every record)"),
    ("--build-summary", true, "same", "the closing `summary` record (`--no-build-summary` turns it off)"),
    ("--stable-diagnostics", true, "same", "deterministic output: fixed record order, no progress or status lines, no colour (`--no-stable-diagnostics`)"),
    ("--test-report", true, "test", "`test`, `test-suite` and failure `diagnostic` records from libtest's text (`--no-test-report`)"),
    ("--ownership-trace", false, "same", "add an `ownership_trace` (declared, moved/borrowed, conflict, later use) to borrow-checker and lifetime errors"),
    ("--fix-preview", false, "same", "a `fix-preview` record: MachineApplicable suggestions as a dry-run unified diff; writes nothing"),
    ("--explain-missing-crate", false, "same", "a `missing-crate` record after each unresolved import or unlinked crate: crate, Cargo.toml line, feature; offline"),
    ("--explain-rebuild=NAME", false, "same", "a `why-rebuilt` record: why cargo rebuilt (or reused) crate NAME; repeatable"),
    ("--explain-feature=NAME", false, "any", "a `why-feature` record from `cargo tree -e features -i NAME`; repeatable; works on its own"),
    ("--panic-report", false, "run test", "a `panic-report` record per panic with the backtrace cut to your own crates; sets RUST_BACKTRACE=1 unless you set it"),
    ("--agent-detail=LEVEL", false, "any", "how much each record says: brief, normal (default in agent mode) or full; also RUSTY_AGENT_DETAIL=LEVEL, the flag wins"),
    ("--agent-help", false, "-", "print this description as one JSON object on standard output (works without agent mode)"),
];

pub fn agent_section() -> String {
    let mut s = String::from("\nAgent mode (rusty-cargo; off unless RUSTY_AGENT=1 or --agent-mode; see AGENT-MODE.md):\n");
    for (n, umbrella, _, d) in OPTIONS { s.push_str(&format!("      {n:<28} {d}{}\n", if *umbrella { " [on in --agent-mode]" } else { "" })); }
    s
}

pub fn agent_help() -> Value {
    let options: Vec<Value> = OPTIONS.iter().map(|(n, u, a, d)| json!({"name": n, "in_agent_mode": u, "applies_to": a, "description": d})).collect();
    json!({
        "schema": 1, "tool": "rusty-cargo", "version": env!("CARGO_PKG_VERSION"),
        "what": "a driver in front of cargo: without agent mode it is a pass-through (same bytes, same exit code); in agent mode it adds JSON-line records on stderr",
        "turn_on": ["RUSTY_AGENT=1", "--agent-mode"], "turn_off": ["--no-agent-mode"],
        "version_line": "agent mode: yes, schema 1 (second line of --version, only in agent mode)",
        "environment": [
            {"name": "RUSTY_AGENT=1", "description": "same as --agent-mode (any other value changes nothing)"},
            {"name": "RUSTY_AGENT_DETAIL=brief|normal|full", "description": "detail level of the records (default normal). brief: code, message, one location, one fix, counts. normal: what is needed to act; no `rendered` text, no empty fields, no repeated facts, passing tests only as counts, lists capped with `<list>_omitted` counts, at most about 20 KB of records on stderr per command (the summary says what was cut; RUSTY_AGENT_OUT keeps everything). full: every field, as the first version wrote them."},
            {"name": "RUSTY_AGENT_OUT=PATH", "description": "agent mode only: also append every JSON record to PATH, one object per line, written as it is produced (stderr still gets them). An empty path is refused with a note; PATH that cannot be opened is reported once and ignored. Never written when agent mode is off. Gives a clean channel when a harness merges stdout and stderr: the file holds only records."},
            {"name": "RUSTY_AGENT_FD=N", "description": "agent mode only: also append every JSON record to the already-open descriptor N (3 or more, open for writing: a pipe, file or terminal the caller left open), one object per line, written as it is produced (stderr still gets them). With RUSTY_AGENT_OUT both get every record. A bad, closed or read-only descriptor gives one warning record, code R0500, and the command goes on with its exit code unchanged. Cargo and the programs it runs do not inherit the variable. Ignored when agent mode is off."}
        ],
        "dropped_lines": "in agent mode with the summary on, cargo's closing lines that records already carry are not printed (`error: could not compile ...`, `warning: build failed, waiting ...`; with the test report also `error: test failed, to rerun pass ...`, `error: doctest failed ...`, `error: N targets failed:` and its list); the summary counts them in human_lines_dropped. libtest text on stdout and the program's output are never touched.",
        "options": options,
        "records": [
            {"type": "diagnostic", "fields": ["schema", "type", "code", "severity", "message", "rustc_code", "flag", "package", "target", "root", "files", "symbols", "notes", "suggestions", "locations", "fixits", "rendered (full only)", "type_mismatch?", "suppressed?", "follow_on?", "ownership_trace?", "<list>_omitted?", "more?"]},
            {"type": "summary", "fields": ["human_lines_dropped?", "exit", "elapsed_ms (not in stable mode)", "command (full only)", "records_omitted? (size budget)", "units{compiled,fresh,failed,times[{package,target,kind,fresh,failed?,ms?}]}", "diagnostics{errors,fatal,warnings,notes,by_lint,by_rustc_code,by_code}", "siblings{errors,warnings}", "tests?", "status_lines_consumed? (full only)"]},
            {"type": "test", "fields": ["binary", "name", "status", "should_panic", "ignore_reason?", "panic?", "output", "output_truncated", "rendered", "duration_ms"]},
            {"type": "test-suite", "fields": ["binary", "expected", "complete", "result", "passed", "failed", "ignored", "measured", "filtered_out", "finished_in_s"]},
            {"type": "fix-preview", "fields": ["dry_run", "written", "rounds", "files[{file,root,safe_edits,unsafe_edits,conflicts,diff,diff_with_unsafe}]", "edits", "apply"]},
            {"type": "missing-crate", "fields": ["situation", "name", "path", "location", "package", "manifest", "section", "found", "features", "cargo_toml_line", "add_command", "near_misses", "network", "suggestions"]},
            {"type": "why-rebuilt", "fields": ["query", "found", "units[{package,version,verdict,reason,reason_kind,details,caused_by?,fingerprint_diff?,fingerprint_log}]"]},
            {"type": "why-feature", "fields": ["query", "found", "kind (package or feature)", "enabled? (feature)", "hint, close_names (found false)", "packages", "required_by", "features[{feature,enabled_by[{chain}]}]", "defined_but_off? (feature)", "command, tree, rendered (full only)"]},
            {"type": "panic-report", "fields": ["id", "source", "test", "thread", "message", "location", "backtrace_found", "frames_total", "frames_shown", "frames", "rendered (full only)"]},
        ],
        "codes": [
            {"code": "R0001", "meaning": "unresolved name"}, {"code": "R0002", "meaning": "syntax error"}, {"code": "R0003", "meaning": "unresolved import or missing crate"},
            {"code": "R0004", "meaning": "type mismatch"}, {"code": "R0005", "meaning": "trait or method problem"}, {"code": "R0006", "meaning": "ownership and borrowing"},
            {"code": "R0007", "meaning": "lifetime problem"}, {"code": "R0008", "meaning": "wrong number or kind of arguments or generics"}, {"code": "R0009", "meaning": "unused or dead code"},
            {"code": "R0010", "meaning": "deprecated"}, {"code": "R0011", "meaning": "clippy lint"}, {"code": "R0012", "meaning": "privacy"}, {"code": "R0013", "meaning": "missing items (match arms, trait items, fields)"},
            {"code": "R0014", "meaning": "linker failure"}, {"code": "R0015", "meaning": "other rustc lint"}, {"code": "R0400", "meaning": "a test failed"}, {"code": "R0500", "meaning": "agent-mode setup problem (RUSTY_AGENT_FD cannot be used; a warning)"},
            {"code": "R0100", "meaning": "other error"}, {"code": "R0200", "meaning": "other warning"}, {"code": "R0300", "meaning": "fatal (internal compiler error)"},
        ],
        "rules": [
            "default behaviour is a pass-through: same exit code, same bytes, same artifacts",
            "records are JSON lines on standard error, each with schema 1 and a type; standard output is never changed",
            "rustc's own error code is in rustc_code (E0308); the lint name is in flag; the R code is chosen from those, never from the message text",
            "rendered (full only) is exactly what rustc printed; followers of a root cause are listed in follow_on (with their rendered text at full)",
            "options after a bare -- belong to rustc, the test harness or your program and are never read",
            "LC_ALL is not set: it would change what build scripts and programs see",
            "records also go to the file named by RUSTY_AGENT_OUT and to the descriptor RUSTY_AGENT_FD, in agent mode only",
            "an absent field means empty, zero or null (normal and brief); a backtrace appears once, in the panic-report, and the test record and R0400 diagnostic point to it with backtrace_in",
            "RUSTY_AGENT=1 is passed on to children, so rusty-cc and rusty-ld write their own JSON lines, which pass through untouched",
        ],
        "not_built": ["macro expansion view at a span: needs -Zunpretty=expanded (nightly or RUSTC_BOOTSTRAP) and the output has no span map; macro call chains are in diagnostic locations[].expansion instead"],
    })
}
