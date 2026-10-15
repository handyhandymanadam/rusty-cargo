# Agent mode (rusty-cargo)

`rusty-cargo` is a driver in front of cargo (the first `cargo` on PATH that is not itself, or `RUSTY_CARGO_REAL`). Without agent mode it
replaces itself with cargo: same arguments (`cargo +1.98.1 ...` too), environment, standard streams, exit code, output bytes and build
artifacts. Install it as `cargo` earlier on PATH, or call it as `rusty-cargo`. Agent mode is for programs and scripts that read the
build output. It is off unless asked for: `--agent-mode` (removed before cargo sees the arguments) or `RUSTY_AGENT=1` (any other value
changes nothing; `--no-agent-mode` turns it off). `--version` prints cargo's text unchanged and, only in agent mode, the line
`agent mode: yes, schema 1`. `--help` in agent mode is cargo's help followed by a section on these options. `RUSTY_AGENT=1` is passed on
to cargo's children, so rusty-cc and rusty-ld (if used as cc or linker) write their own JSON lines; those pass through untouched.
Nothing after a bare `--` is read (it belongs to rustc, the test harness or your program).

Records are JSON, one per line, on standard error. Every record starts with `"schema": 1` and a `"type"`. The format is shared with
rusty-cc, rusty-ld, rusty-grep and rusty-sh. Standard output is never changed (the program's output and libtest's text pass through
byte for byte). Agent mode runs cargo with `--message-format=json` for build, check, clippy, test, bench, run, doc, rustc and rustdoc (other
subcommands, and any command line that already has its own `--message-format`, are plain pass-through). Artifacts are byte-identical to
plain cargo (tests compare hashes).

| Flag | What | In `--agent-mode` |
|---|---|---|
| `--diagnostics-format=json` | every compiler and clippy message as a `diagnostic` record; `=text` gives cargo's own text back | yes |
| `--root-cause` / `--no-root-cause` | group follow-on errors under the first cause | yes |
| `--build-summary` / `--no-build-summary` | the closing `summary` record | yes |
| `--stable-diagnostics` / `--no-stable-diagnostics` | deterministic output (see below) | yes |
| `--test-report` / `--no-test-report` | `test`, `test-suite` records from libtest's text, failures also as `diagnostic` R0400 | yes |
| `--ownership-trace` | `ownership_trace` on borrow-checker and lifetime errors | no |
| `--fix-preview` | a `fix-preview` record, a dry-run diff; writes nothing | no |
| `--explain-missing-crate` | a `missing-crate` record per unresolved import / unlinked crate | no |
| `--explain-rebuild=NAME` | a `why-rebuilt` record for crate NAME (repeatable) | no |
| `--explain-feature=NAME` | a `why-feature` record (repeatable; works without a subcommand) | no |
| `--panic-report` | a `panic-report` record per panic, frames cut to your crates | no |
| `--agent-detail=brief\|normal\|full` | how much each record says (also `RUSTY_AGENT_DETAIL`; the flag wins; default `normal`) | normal |
| `--agent-help` | options, records, codes and rules as one JSON object on stdout (works without agent mode) | no |

Each flag also works alone. Flags that read diagnostics turn the diagnostic records on. With `--diagnostics-format=text` (or when only
`--build-summary` etc. is asked) cargo's own text is printed exactly as plain cargo prints it. None of the names is an option of cargo, any
cargo subcommand, rustc, `-Z`, `-C` or `-W` help (tested).

## Detail levels and the size budget (`--agent-detail=`, `RUSTY_AGENT_DETAIL=`)
Agent mode is for programs and scripts that pay for every byte, so records come in three sizes. An absent field means empty, zero or null.
- `full`: every field, as the first version wrote them (`rendered`, `files`, `symbols`, null fields, the summary's `command`, all passing
  tests, all lists). Keys start with `schema`, `type` (diagnostics continue `code`, `severity`, `message`).
- `normal` (default): what is needed to act. No `rendered` (rustc's own text, which repeats message, labels and notes; use `full` to see it),
  no `files`, `symbols`, `root` (when it is the current directory), empty or null fields, `end_line` when it equals `line`, note locations
  that a `notes` entry already names, a type mismatch's own copy of the primary location and label, fix-its' back pointers. Passing and
  ignored tests are counts in `test-suite` (a `test` record only for failures and benches). The summary has no `command`, no unit list on
  success, and only failed or compiled units otherwise. Lists are capped (`notes` 4, `locations` 8, `suggestions` 3, `follow_on` 5, summary
  units 8, `by_*` 10, panic frames 12) with a `<list>_omitted` count and a `more` pointer. Ownership events point into `locations` with `loc`
  (an index) and keep a label only when it differs from the location's own.
- `brief`: code, severity, message, rustc code and lint name, one `location`, one `fix` (message, applicability, edits), counts of notes and
  suggestions; tests only as failures with the message; the summary without `by_*`.
Size budget (about 5 KB for a typical command): at `normal` and `brief`, once the records on stderr pass 12 KB only errors, failed tests and
requested records (`--explain-*`, `--fix-preview`, `--panic-report`) go on, and past 20 KB nothing but the summary. The summary then has
`records_omitted` with counts per type and a pointer. `RUSTY_AGENT_OUT` always gets every record (shaped, without the budget); `full` has no
cap. `tests/sizes.py --check` and `tests/agent_detail.py` fail when a record type or a job grows past its budget.

## Records file: `RUSTY_AGENT_OUT=PATH`
Harnesses often merge standard output and standard error, so records get tangled with cargo's text and libtest's text. With agent mode on
and `RUSTY_AGENT_OUT` set, every record is also appended to that file, one JSON object per line, written as the record is produced (stderr
still gets the same records). The file holds only records (including the JSON lines rusty-cc and rusty-ld write, which pass through to
stderr as before). It is opened in append mode (a second run adds to it); an empty path is refused with a one-line note, a path that cannot
be opened is reported once and ignored, and the build goes on. It is never written when agent mode is off (including `--no-agent-mode` over
`RUSTY_AGENT=1`, and feature flags used without agent mode).

## Records descriptor: `RUSTY_AGENT_FD=N`
Same records, same order, same precedence as `RUSTY_AGENT_OUT`, appended to descriptor `N`, which the caller opened and left open (for example
`3>records.jsonl`, or a pipe it reads). It is re-opened through `/proc/self/fd/N` as a new append handle: the caller's descriptor is untouched and
there is no unsafe code. `N` must be a decimal number of 3 or more and open for writing. If `RUSTY_AGENT_OUT` is also set, both get every record.
A value that is not a number, a closed descriptor or a read-only one gives one `diagnostic` record, code `R0500`, severity `warning`, on stderr
(and on the channel that works); the build and its exit code do not change. Cargo, rustc, rusty-cc and rusty-ld do not inherit the variable (this
process copies their records), except when agent mode runs a plain pass-through exec. Ignored when agent mode is off: plain output is unchanged.

## Cargo's closing lines
In agent mode, with the build summary on, cargo's own trailing lines that the records already carry are not printed: `error: could not
compile `x` ... due to N previous errors` (the summary has `exit` and `units.failed`) and `warning: build failed, waiting for other jobs to
finish...`; with the test report on also `error: test failed, to rerun pass ...`, `error: doctest failed, to rerun pass ...` and
`error: N targets failed:` with its list (the `test-suite` records say which failed). `summary.human_lines_dropped` counts them. Turning
off `--no-build-summary` (or `--no-test-report` for the test lines) keeps them. libtest's text on stdout, the program's own output and every
other cargo line are unchanged; with agent mode off nothing is dropped. (`warning: ... generated N warnings` does not exist under
`--message-format=json`, so there is nothing to drop.)

## Record `diagnostic`
`schema`, `type`, `code` (R-prefixed, below), `severity` (`fatal`, `error`, `warning`, `note`), `message`, `rustc_code` (`E0308` or null),
`flag` (the lint name, `unused_variables` or `clippy::len_zero`, or null), `package`, `target` (`name`, `kind`), `root` (the directory the
relative file names are relative to), `files`, `symbols` (the backticked names in the message), `notes` (flattened children as text:
`file:line:col: note: ...`), `suggestions` (`{message, option, applicability, fixits}`; `fixits` are indexes), `locations` (`{file, line,
column, end_line, end_column, role, label}`; `role` is `primary`, `secondary` or `note`; `macro` and `expansion` (the macro call chain,
innermost first) when the span comes out of a macro), `fixits` (`{file, start{line,column,offset}, end{...}, text, applicability,
suggestion}`; offsets are bytes; a multi-part suggestion shares its `suggestion` index), `rendered` (exactly what rustc printed, once).
With `--root-cause`: `suppressed` and `follow_on` (`{code, severity, message, rustc_code, flag, location, rule, rendered}`).
`type_mismatch` (E0308, E0053, E0271): `{expected, found, expected_kind, found_kind, location, source (label or note), heuristic}`.
It reads rustc's own label ``expected `i32`, found `&str` `` (heuristic false) or the two-line note (heuristic true).
Clippy lints are the same record, `flag` = `clippy::name`, code `R0011`.

Codes are set at an explicit match arm from `rustc_code` or the lint name (never from message wording; the one exception is a syntax error,
which rustc gives no code, recognised by the start of its message): `R0001` unresolved name, `R0002` syntax error, `R0003` unresolved
import or missing crate, `R0004` type mismatch, `R0005` trait or method problem, `R0006` ownership and borrowing, `R0007` lifetime,
`R0008` wrong number or kind of arguments, `R0009` unused or dead code, `R0010` deprecated, `R0011` clippy lint, `R0012` privacy,
`R0013` missing items, `R0014` linker, `R0015` other rustc lint, `R0400` a test failed, `R0500` agent-mode setup problem (`RUSTY_AGENT_FD`, a warning); fallbacks `R0100` error, `R0200` warning,
`R0300` fatal.

## Root cause rules (`follow_on[].rule`)
`same-place` (the identical diagnostic at the same place, any severity), `same-unresolved-name` (the same unresolved name again),
`mentions-unresolved-name` (a later error names a name that was unresolved). Warnings are only grouped by `same-place`. The followers'
own records are not written; the leader lists them with their full `rendered` text, and `summary` counts every one.

## Stable mode
Records are written after the build finishes, units in (package, kind, target) order, diagnostics within a unit sorted by (file, line,
column, code, message) (root-cause grouping is decided first, in rustc's own order); cargo's status lines (`Compiling`, `Finished`, `Running`)
are not printed (the summary counts them in `status_lines_consumed`); `CARGO_TERM_COLOR=never`, no progress bar; tests inside a binary are
sorted by name. Nothing that differs between runs or machines: no thread ids (`thread 'main' (4242) panicked` becomes `thread 'main' panicked`),
no `note: run with RUST_BACKTRACE` line, no random doc-test directory (`<tmp>/`), no `elapsed_ms`, per-unit `ms` or `finished_in_s`, and a test
binary's `path` is just its name. Without `--stable-diagnostics` the clock values come back (a fresh unit never has an `ms`). `LC_ALL` is deliberately not set: it would change what
build scripts and `cargo run` programs see, and rustc's messages do not depend on it (tested with another locale).

## Record `summary`
`exit`, `elapsed_ms`, `command` (the cargo command line actually run, with `--message-format=json`), `units` (`compiled`, `fresh`, `failed`
(from cargo's "could not compile"), `times` (`package`, `target`, `kind`, `fresh`, `ms`: from cargo's `Compiling` line to its finished
artifact message, so it includes waiting for dependencies)), `diagnostics` (`errors`, `fatal`, `warnings`, `notes`, `by_lint`,
`by_rustc_code`, `by_code`; totals include followers hidden by `--root-cause`), `siblings` (error and warning counts of JSON lines written
by rusty-cc and rusty-ld that passed through), `tests` (when tests ran).

## `ownership_trace` (`--ownership-trace`)
For E0382, E0499, E0502, E0503, E0505, E0506, E0597, E0716, E0515, E0373, E0507, E0596, E0594, E0384, E0381, E0521 and the common lifetime
errors: `kind` (`use-after-move`, `two-mutable-borrows`, `conflicting-borrow`, `move-while-borrowed`, `assign-while-borrowed`,
`borrowed-value-dropped`, ...), `variable`, `type` (from "has type `T`"), `events` (`order`, `role`, `location`, `label`, `source` (span or
child-note), `heuristic`), `complete`, `missing`. Roles: `declared`, `moved`, `borrowed`, `borrowed_value`, `takes_ownership`,
`conflicting_borrow`, `conflicting_move`, `conflicting_assign`, `conflicting_use`, `use_after_move`, `dropped`, `temporary_created`,
`temporary_freed`, `return_of_local`, `later_use`; anything unrecognised is `related` with `heuristic: true`. Order is by role
(declaration, then the earlier use, the conflict, the later use), then by position; it is a reading order, not an execution trace (a loop
can run the "later" use first). The roles come from rustc's label wording, so they are exact for current rustc and need a check after a
rustc upgrade. For errors where rustc's JSON has no span for a relation (the ones listed with `complete: false`), `missing` names it and
`note` says so; a rustc patch is out of scope.

## `fix-preview`
`dry_run`, `written` (always false), `rounds` (1; `cargo fix` repeats rounds until nothing changes), `files` (`file`, `root`, `safe_edits`,
`unsafe_edits`, `conflicts`, `diff`, `diff_with_unsafe`), `edits` (`file`, offsets, `text`, `safety`, `applicability`, `message`, `code`, `from`,
`in_diff`, `in_diff_with_unsafe`). Safe = every part of the suggestion is `MachineApplicable`; unsafe = `MaybeIncorrect`,
`HasPlaceholders` or `Unspecified`. A suggestion is applied whole or not at all; overlapping suggestions are counted as `conflicts` and
skipped. The safe diff applied with `patch -p1` from `root` equals the files `cargo fix` writes for the first round (tested byte for byte).

## `missing-crate` (`--explain-missing-crate`)
After each E0432, E0433 or E0463 about a crate: `situation` (`feature-gated`, `in-lockfile-not-a-dependency`, `in-registry-cache`,
`workspace-member`, `optional-dependency-off`, `wrong-dependency-kind`, `declared-but-unlinked`, `not-a-dependency`), `name`, `section` (the
Cargo.toml table by target kind), `found` (`in_manifest`, `optional`, `locked_version`, `cached_versions`, `workspace_member`, `path`),
`features` (the feature, with rustc's own "gated behind the `x` feature" label as `evidence`), `cargo_toml_line`, `add_command` (always
`--offline` where it can be), `near_misses` (names within edit distance 2), `network` (`not used`). Sources: `cargo metadata --offline`
(which reads Cargo.lock), the crates already in `~/.cargo/registry/cache`. It never starts a download.

## `test`, `test-suite` (`--test-report`)
Parsed from stable libtest's text (no JSON exists on stable; nextest is not needed). `test`: `binary` (`kind` unittests, integration,
doc-tests, bench; `target`, `path`, `package`), `name`, `status` (`ok`, `failed`, `ignored`, `bench`), `should_panic`, `ignore_reason`,
`panic` (`thread`, `message`, `file`, `line`, `column`, `assertion` (`left == right` with `left`, `right`, `custom`; or `assert` with `expr`),
`kind: did-not-panic`, `file_is_temporary` for doc tests), `output` (what the test printed, trimmed to 4000 bytes), `rendered` (libtest's
failure text), `duration_ms` (always null: stable libtest has no per-test times; `finished_in_s` is per binary). `test-suite`: counts and
`finished_in_s` (not in stable mode) from libtest's summary line. A failed test also gets a `diagnostic` with code `R0400`: a doc test's primary location is the start of its code block (the line in its
name, role `primary`); when the panic happened in library code the doc test called, that place is added with role `secondary`. With
`--panic-report` the backtrace is written once, in the `panic-report` record (it has an `id`); the test record and the R0400 diagnostic say
`backtrace_in` with that id and keep only the text before the backtrace. Several test binaries, doc tests, `--no-fail-fast`, `--show-output`, `--nocapture` are covered by tests.

## `why-rebuilt` (`--explain-rebuild=NAME`)
Runs the build with `-v` and `CARGO_LOG=cargo::core::compiler::fingerprint=info` and reads cargo's `Dirty` / `Fresh` lines and the
fingerprint log (neither is printed to you). Per matching unit: `verdict` (`rebuilt`, `fresh`), `reason` (cargo's words), `reason_kind`
(`file-changed`, `env-changed`, `dependency-rebuilt` with `caused_by` chain, `rustflags-changed`, `profile-changed`, `features-changed`,
`no-previous-fingerprint`, `other`), `details`, `fingerprint_log`. When there is no earlier fingerprint (flags, features or profile differ)
the fingerprint files of the same crate are compared and `fingerprint_diff.fields` names the field (`features`, `rustflags`, ...). `reason` has
cargo's words without the clock values in its parenthesis; `fingerprint_log` leaves out the lines that repeat the reason or are raw Rust Debug text
(`FileTime { ... }`).

## `why-feature` (`--explain-feature=NAME`)
NAME can be a package or a feature. A feature name (not a package) is looked up in `cargo metadata --offline`: `kind: "feature"`, `enabled`,
`packages[{package, enabled_by[{chain}]}]` (the packages where the feature is on) and `defined_but_off`; an unknown name gives `found: false`,
a `hint` (NAME must be a package or a feature) and `close_names`. For a package (`kind: "package"`): `cargo tree -e features -i NAME` with the same `-p`, `--features`, `--manifest-path`, `--target`, ... as your command line: `required_by`,
`features[{feature, enabled_by[{chain}]}]` (each chain ends at a command-line feature or the root package; cargo's `(*)` copies are
followed through the full copy), the parsed `tree` and cargo's text in `rendered`. With another subcommand that command runs as usual
first. Alone (`rusty-cargo --explain-feature=NAME --features x`) only the record is written.

## `panic-report` (`--panic-report`)
For `run`, `test`, `bench`: sets `RUST_BACKTRACE=1` for the build's children unless you have set it (this changes the program's own stderr:
it prints a backtrace). Per panic: `source` (`run` or `test`), `test`, `thread`, `message`, `location`, `backtrace_found`, `frames_total`,
`frames_shown`, `frames` (`index`, `function`, `file`, `line`, `column`; only frames in workspace members and path dependencies, i.e.
packages whose id is `path+file://...`), and `rendered` (the whole text incl. std frames). A program that was stripped of debug info is
matched by crate name in the function path.

## Not built: macro expansion at a span (feature 11)
Tried twice, then skipped (rule: only if rustc's unstable expand is not needed). Try 1: `cargo rustc -- -Zunpretty=expanded` on stable
1.98.1 is refused ("the option `Z` is only accepted on the nightly compiler"). Try 2: with `RUSTC_BOOTSTRAP=1` it works, but that is the
unsupported bypass, and the output is the whole expanded file with no span map, so "the expansion at FILE:LINE:COL" cannot be located
reliably. What stable rustc does give is in every diagnostic: `locations[].expansion`, the chain of macro calls (call site and definition
site) a span came out of.

## Tests
`cargo +1.98.1 test --release -j4` runs 12 python scripts against the built binary on real cargo jobs (fixture crates in tests/fixtures,
copied to a private temp dir per script, one script at a time) and parses the JSON in python. `tests/baseline.py` recorded the plain-cargo
baseline (baseline/baseline.json).
