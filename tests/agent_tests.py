#!/usr/bin/env python3
"""Feature 7: test results as records (unit, integration and doc tests, several binaries); libtest's text is untouched on stdout."""
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
fails = 0
def check(name, cond, detail=""):
    global fails
    if not cond: fails += 1
    print(("ok   " if cond else "FAIL ") + name + ("" if cond else "  " + str(detail)[:700]))
tmp, fx = private_fixtures("testmulti", "testfail", "testpass", "errors3")
J = ["-j4", "--offline"]
def go(fixture, flags=("--agent-mode",), tail=("--test-threads=1",), sub="test", extra=()):
    tgt = tempfile.mkdtemp(dir=tmp)
    args = [sub] + list(extra) + list(flags) + J + (["--"] + list(tail) if tail else [])
    code, out, err = run([RC, TOOLCHAIN], args, fx[fixture], target=tgt)
    return code, out, records(err)[0], tgt
code, out, recs, tgt = go("testmulti", extra=("--no-fail-fast",))
tests = {(r["binary"]["kind"], r["name"]): r for r in recs if r["type"] == "test"}
suites = [r for r in recs if r["type"] == "test-suite"]
check("three binaries: unit tests, integration test file, doc tests", [s["binary"]["kind"] for s in suites] == ["unittests", "integration", "doc-tests"], [s["binary"] for s in suites])
check("suite counts match libtest's own summary lines", [(s["passed"], s["failed"], s["ignored"]) for s in suites] == [(1, 2, 1), (1, 2, 0), (0, 1, 0)], suites)
check("exit code 101 as cargo", code == 101)
ok = tests[("unittests", "tests::inc_works")]
check("passing test: status ok, no panic, null duration (stable libtest has none)", ok["status"] == "ok" and "panic" not in ok and ok["duration_ms"] is None)
ig = tests[("unittests", "tests::slow")]
check("ignored test keeps its reason", ig["status"] == "ignored" and ig["ignore_reason"] == "too slow", ig)
un = tests[("unittests", "tests::unwrap_none")]
check("panic: message, file:line:column, thread", un["status"] == "failed" and un["panic"]["message"] == "called `Option::unwrap()` on a `None` value" and (un["panic"]["file"], un["panic"]["line"], un["panic"]["column"]) == ("src/lib.rs", 18, 53) and un["panic"]["thread"] == "tests::unwrap_none", un.get("panic"))
sp = tests[("unittests", "tests::should_have_panicked")]
check("#[should_panic] that did not panic", sp["should_panic"] and sp["status"] == "failed" and sp["panic"]["kind"] == "did-not-panic" and sp["panic"]["line"] == 16, sp)
a = tests[("integration", "integration_assert")]
check("assert!: expression captured, integration file path", a["panic"]["assertion"] == {"kind": "assert", "expr": "1 + 1 == 3"} and a["panic"]["file"] == "tests/it.rs" and a["panic"]["line"] == 5, a["panic"])
m = tests[("integration", "integration_message")]
check("panic! with formatted message", m["panic"]["message"] == "nope: 42", m["panic"])
d = tests[("doc-tests", "src/lib.rs - inc (line 1)")]
check("doc test failure: assert_eq left/right and a note that the file is temporary", d["status"] == "failed" and d["panic"]["assertion"]["left"] == "2" and d["panic"]["assertion"]["right"] == "3" and d["panic"]["assertion"]["custom"] == "doc says three" and d["panic"]["file_is_temporary"], d["panic"])
diag = [r for r in recs if r["type"] == "diagnostic" and r.get("test") == "src/lib.rs - inc (line 1)"][0]
check("failed test also as a diagnostic R0400 pointing at the doc test's place", diag["code"] == "R0400" and diag["severity"] == "error" and diag["locations"][0]["file"] == "src/lib.rs" and diag["locations"][0]["line"] == 1 and diag["rendered"], diag)
failed_diags = [r for r in recs if r["type"] == "diagnostic"]
check("one R0400 diagnostic per failed test (5)", len(failed_diags) == 5 and all(r["code"] == "R0400" for r in failed_diags), len(failed_diags))
check("summary carries the totals", recs[-1]["type"] == "summary" and recs[-1]["tests"] == {"suites": 3, "passed": 2, "failed": 5, "ignored": 1, "filtered_out": 0}, recs[-1].get("tests"))
# the text on stdout is exactly what plain cargo prints
tgt2 = tempfile.mkdtemp(dir=tmp)
c2, o2, e2 = run(["cargo", TOOLCHAIN], ["test", "--no-fail-fast"] + J + ["--", "--test-threads=1"], fx["testmulti"], target=tgt2)
check("stdout is byte-identical to plain cargo's (libtest text untouched)", norm(out, tgt) == norm(o2, tgt2), (norm(out, tgt)[:200], norm(o2, tgt2)[:200]))
check("exit code same as plain cargo", code == c2)
# parallel tests: stable mode sorts tests by name
code, out, recs, tgt = go("testmulti", tail=(), extra=("--no-fail-fast",))
names = [r["name"] for r in recs if r["type"] == "test" and r["binary"]["kind"] == "unittests"]
check("stable: tests sorted by name even when run in parallel", names == sorted(names), names)
# --nocapture and --show-output
code, out, recs, tgt = go("testmulti", tail=("--test-threads=1", "--show-output"), extra=("--no-fail-fast",))
ok = [r for r in recs if r["type"] == "test" and r["name"] == "tests::inc_works"][0]
check("--show-output: a passing test's captured output is in the record", ok["output"] == "hello from inc_works", ok)
code, out, recs, tgt = go("testmulti", tail=("--test-threads=1", "--nocapture"), extra=("--no-fail-fast",))
check("--nocapture: results still found", {r["name"]: r["status"] for r in recs if r["type"] == "test" and r["binary"]["kind"] == "unittests"}["tests::inc_works"] == "ok" and b"hello from inc_works" in out)
# passing run
code, out, recs, tgt = go("testpass")
check("all passing: exit 0, no diagnostics, 2 suites (unit + doc)", code == 0 and not any(r["type"] == "diagnostic" for r in recs) and [s["binary"]["kind"] for s in recs if s["type"] == "test-suite"] == ["unittests", "doc-tests"])
# build failure: no test records, no crash
code, out, recs, tgt = go("errors3", tail=())
check("compile error: diagnostics and summary, no test records", code == 101 and not any(r["type"] in ("test", "test-suite") for r in recs) and recs[-1]["type"] == "summary" and "tests" not in recs[-1])
# alone, and off switch
code, out, recs, tgt = go("testfail", flags=("--test-report",))
check("--test-report alone: test records without diagnostics-format or summary", any(r["type"] == "test" for r in recs) and not any(r["type"] == "summary" for r in recs))
code, out, recs, tgt = go("testfail", flags=("--agent-mode", "--no-test-report"))
check("--no-test-report removes them from the umbrella", not any(r["type"] in ("test", "test-suite") for r in recs))
code, out, recs, tgt = go("testfail", flags=())
check("nothing without agent mode", recs == [])
print("FAILED" if fails else "ALL PASS"); sys.exit(1 if fails else 0)
