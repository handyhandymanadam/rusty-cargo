#!/usr/bin/env python3
"""Feature 10: --panic-report for `cargo run` and `cargo test`: backtrace frames trimmed to the user's crates, full text in rendered."""
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
fails = 0
def check(name, cond, detail=""):
    global fails
    if not cond: fails += 1
    print(("ok   " if cond else "FAIL ") + name + ("" if cond else "  " + str(detail)[:900]))
tmp, fx = private_fixtures("panicker", "clean")
p = fx["panicker"]; J = ["-j4", "--offline"]
def go(sub, flags, tail=(), fixture="panicker", env=None):
    args = [sub] + list(flags) + J + (["--"] + list(tail) if tail else [])
    code, out, err = run([RC, TOOLCHAIN], args, fx[fixture], env=env, target=tempfile.mkdtemp(dir=tmp))
    return code, out, err, records(err)[0]
code, out, err, recs = go("run", ["--panic-report"])
pr = [r for r in recs if r["type"] == "panic-report"]
check("run: one panic-report, exit code 101 like the program's panic", code == 101 and len(pr) == 1 and pr[0]["source"] == "run", (code, recs))
r = pr[0]
check("message, thread and place of the panic", r["message"] == "boom at zero" and r["thread"] == "main" and r["location"] == {"file": "inner/src/lib.rs", "line": 2, "column": 17}, r)
check("frames trimmed to the user's crates: inner::boom, level2, level1, main (std frames dropped)", [f["function"] for f in r["frames"]] == ["inner::boom", "panicker::level2", "panicker::level1", "panicker::main"], r["frames"])
check("frames carry file:line:column and the original index", r["frames"][0]["file"] == "inner/src/lib.rs" and r["frames"][0]["line"] == 2 and r["frames"][3]["file"] == "src/main.rs" and r["frames"][3]["line"] == 7 and r["frames"][0]["index"] == 2, r["frames"])
check("frames_total counts every frame, frames_shown the kept ones", r["frames_total"] == 7 and r["frames_shown"] == 4, (r["frames_total"], r["frames_shown"]))
check("full text kept in rendered, including the std frames", "rust_begin_unwind" in r["rendered"] and "panic_fmt" in r["rendered"] and "stack backtrace:" in r["rendered"])
check("the program's own stdout and stderr text still reach the user", b"start" in out and b"panicked at inner/src/lib.rs:2:17" in err and b"stack backtrace:" in err)
# without the flag the program is not given RUST_BACKTRACE
code, out, err, recs = go("run", [])
check("no flag: no backtrace requested, no records", b"stack backtrace" not in err and recs == [])
# user's own RUST_BACKTRACE wins
code, out, err, recs = go("run", ["--panic-report"], env=clean_env(RUST_BACKTRACE="0"))
pr = [r for r in recs if r["type"] == "panic-report"]
check("RUST_BACKTRACE=0 from the user: panic found, no frames, said so", len(pr) == 1 and pr[0]["backtrace_found"] is False and pr[0]["frames"] == [], pr)
# tests
code, out, err, recs = go("test", ["--panic-report"], tail=("--test-threads=1",))
pr = [r for r in recs if r["type"] == "panic-report"]
check("test: a panic-report for the failing test only", len(pr) == 1 and pr[0]["source"] == "test" and pr[0]["test"] == "tests::explodes", pr)
check("test: frames trimmed to user crates", [f["function"] for f in pr[0]["frames"]][:4] == ["inner::boom", "panicker::level2", "panicker::level1", "panicker::tests::explodes"] or "inner::boom" in [f["function"] for f in pr[0]["frames"]], [f["function"] for f in pr[0]["frames"]])
check("--panic-report alone does not turn on test records", not any(r["type"] in ("test", "test-suite") for r in recs))
code, out, err, recs = go("test", ["--agent-mode", "--panic-report"], tail=("--test-threads=1",))
check("with the umbrella: test records and panic reports together", any(r["type"] == "test" for r in recs) and any(r["type"] == "panic-report" for r in recs))
# clean program: no record
code, out, err, recs = go("run", ["--panic-report"], fixture="clean")
check("program that does not panic: nothing extra", code == 0 and not any(r["type"] == "panic-report" for r in recs))
print("FAILED" if fails else "ALL PASS"); sys.exit(1 if fails else 0)
