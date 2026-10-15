# rusty-cargo

A thin driver that sits in front of cargo. Used normally it changes nothing: it hands your command to
cargo unchanged, so the output, exit status and the build artifacts are exactly cargo's. Turn on its
agent mode and it turns compiler, test and cargo output into records a program can read.

## Why it exists

Cargo prints for a person: coloured text, one real error followed by its fallout, test results
buried in free text, and borrow-checker explanations written as prose. A script or tool that reads
that has to parse it by guesswork. rusty-cargo asks cargo for its own machine output
(`--message-format=json`), adds what is missing, and prints JSON, one record per line, in a format shared
with the sibling tools (rusty-ld, rusty-cc, rusty-grep, rusty-sh).

## What it does

- **Default (agent mode off):** a transparent pass-through to cargo, including `cargo +toolchain ...`.
  Tests compare output bytes and build artifacts against plain cargo.
- **Agent mode** (`--agent-mode` or `RUSTY_AGENT=1`): compiler and clippy messages become records,
  follow-on errors are grouped under the first cause, test results become `test` and `test-suite` records
  (failures also as diagnostics), and a closing summary reports the result. Standard output, where the
  program's own output goes, is never changed.
- **On request:** an ownership trace for borrow-checker errors; a fix preview as a diff that writes
  nothing; why a crate was rebuilt; why a feature is on; which crate is missing for an unresolved import;
  and panic reports cut down to your own code.
- **Size control:** `--agent-detail=brief|normal|full` (or `RUSTY_AGENT_DETAIL`) sets how much each
  record says. Records go to standard error; a copy can go to a file (`RUSTY_AGENT_OUT`) or an open
  descriptor (`RUSTY_AGENT_FD`).

The full list of options, record types and codes is in [AGENT-MODE.md](AGENT-MODE.md), and
`rusty-cargo --agent-help` prints it as JSON.

## Build and test

Needs a recent Rust (built and tested with 1.98).

    cargo build --release        # binary: target/release/rusty-cargo
    cargo test                   # integration tests; tests/*.py run the real binary on real input

Call it as `rusty-cargo`, or put it earlier on the path under the name `cargo`; it finds the real cargo
by skipping itself, or from `RUSTY_CARGO_REAL`.

## Status and limits

Early (0.1). The record format is a draft (schema 1) and may still change. A bad `RUSTY_AGENT_OUT` path is
reported as a plain-text line, not a record.

## Licence

No licence is granted. The source is published so it can be read; ask before reusing it. The Rust crates
it depends on keep their own licences (MIT, Apache-2.0 and similar).
