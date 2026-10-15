#!/usr/bin/env python3
"""MASTER follow-up: (1) RUSTY_AGENT_OUT=<file> records channel, (2) cargo's own closing lines dropped in agent mode only."""
import json, os, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
fails = 0
def check(name, cond, detail=""):
    global fails
    if not cond: fails += 1
    print(("ok   " if cond else "FAIL ") + name + ("" if cond else "  " + str(detail)[:700]))
tmp, fx = private_fixtures("errors3", "warns", "testfail", "testmulti", "clean", "cascade")
J = ["-j4", "--offline"]
def go(fixture, args, flags=("--agent-mode",), env=None, out_file=None):
    e = dict(env or clean_env())
    if out_file is not None: e["RUSTY_AGENT_OUT"] = out_file
    code, out, err = run([RC, TOOLCHAIN], list(args) + list(flags) + J, fx[fixture], env=e, target=tempfile.mkdtemp(dir=tmp))
    return code, out, err
def filerecs(path): return [json.loads(l) for l in open(path).read().splitlines()] if os.path.exists(path) else None
# --- (1) the records file
f = os.path.join(tmp, "rec1.jsonl")
code, out, err = go("errors3", ["build"], out_file=f)
srec, other = records(err)
frec = filerecs(f)
check("file has every record, same objects in the same order as stderr", frec == srec and len(frec) == 4, (len(frec or []), len(srec)))
check("every line of the file is one JSON object with schema 1", all(r["schema"] == 1 for r in frec) and open(f).read().endswith("\n"))
check("nothing but records in the file (no cargo text)", all(l.startswith("{") for l in open(f).read().splitlines()))
check("stderr still carries the records too", len(srec) == 4)
code2, out2, err2 = go("errors3", ["build"], out_file=f)
check("append, not truncate: a second run adds its records", len(filerecs(f)) == 8)
# tests: file has test records; libtest text stays on stdout only
f2 = os.path.join(tmp, "rec2.jsonl")
code, out, err = go("testmulti", ["test", "--no-fail-fast"], out_file=f2)
fr = filerecs(f2)
check("test run: test, test-suite, diagnostic and summary records in the file", {"test", "test-suite", "diagnostic", "summary"} <= {r["type"] for r in fr})
check("libtest text is not in the file but still on stdout", b"running 4 tests" in out and "running" not in open(f2).read().replace('"running', ""), out[:80])
# agent mode off -> never write
f3 = os.path.join(tmp, "rec3.jsonl")
code, out, err = go("errors3", ["build"], flags=(), out_file=f3)
check("agent mode off: no file written", not os.path.exists(f3) and records(err)[0] == [])
code, out, err = go("errors3", ["build"], flags=("--fix-preview",), out_file=f3)
check("only a feature flag, agent mode off: still no file", not os.path.exists(f3))
code, out, err = go("errors3", ["build"], flags=("--no-agent-mode",), env=clean_env(RUSTY_AGENT="1"), out_file=f3)
check("--no-agent-mode beats RUSTY_AGENT=1: no file", not os.path.exists(f3))
# RUSTY_AGENT=1 and no flag: file written
f4 = os.path.join(tmp, "rec4.jsonl")
code, out, err = go("errors3", ["build"], flags=(), env=clean_env(RUSTY_AGENT="1"), out_file=f4)
check("RUSTY_AGENT=1 plus RUSTY_AGENT_OUT writes the file", filerecs(f4) == records(err)[0] and len(filerecs(f4)) == 4)
# empty path rejected, bad path survives
code, out, err = go("errors3", ["build"], out_file="")
check("empty path: refused with a note, run still fine, records on stderr", code == 101 and b"RUSTY_AGENT_OUT is empty" in err and len(records(err)[0]) == 4)
code, out, err = go("errors3", ["build"], out_file="/nonexistent-dir/x.jsonl")
check("unwritable path: reported once, exit code and records unchanged", code == 101 and err.count(b"cannot open RUSTY_AGENT_OUT") == 1 and len(records(err)[0]) == 4)
# explain-feature without subcommand also writes to the file
f5 = os.path.join(tmp, "rec5.jsonl")
code, out, err = run([RC, TOOLCHAIN], ["--agent-mode", "--explain-feature=memchr", "--offline"], fx["clean"], env=dict(clean_env(), RUSTY_AGENT_OUT=f5), target=tempfile.mkdtemp(dir=tmp))
check("why-feature record also goes to the file", [r["type"] for r in (filerecs(f5) or [])] == ["why-feature"], filerecs(f5))
# --- (2) cargo's closing lines
code, out, err = go("errors3", ["build"])
srec, other = records(err)
check("agent mode: no 'could not compile' line; summary has the exit code and failed unit", not any("could not compile" in o for o in other) and srec[-1]["exit"] == 101 and srec[-1]["units"]["failed"] == ["errors3"], other)
check("the summary counts the dropped lines", srec[-1]["human_lines_dropped"] == 1, srec[-1].get("human_lines_dropped"))
check("rustc's own text is still in rendered (nothing lost)", all("error[" in r["rendered"] for r in srec if r["type"] == "diagnostic"))
code, out, err = go("warns", ["build"])
srec, other = records(err)
check("warnings: cargo prints no 'generated N warnings' line under --message-format=json; warnings are counted in the summary", not any("generated" in o for o in other) and srec[-1]["diagnostics"]["warnings"] == 3 and srec[-1]["human_lines_dropped"] == 0, other)
code, out, err = go("testmulti", ["test", "--no-fail-fast"], flags=("--agent-mode",))
srec, other = records(err)
check("test run: 'error: test failed, to rerun' dropped; suites in records say which failed", not any("to rerun pass" in o for o in other) and [s["failed"] for s in srec if s["type"] == "test-suite"] == [2, 2, 1] and srec[-1]["human_lines_dropped"] == 7, (other, srec[-1].get("human_lines_dropped")))
# libtest stdout and program output are unchanged
tgt = tempfile.mkdtemp(dir=tmp)
c2, o2, e2 = run(["cargo", TOOLCHAIN], ["test", "--no-fail-fast"] + J + ["--", "--test-threads=1"], fx["testmulti"], target=tgt)
c1, o1, e1 = run([RC, TOOLCHAIN], ["test", "--no-fail-fast", "--agent-mode"] + J + ["--", "--test-threads=1"], fx["testmulti"], target=tempfile.mkdtemp(dir=tmp))
check("stdout byte-identical to plain cargo; exit code the same", norm(o1) == norm(o2) and c1 == c2 == 101)
code, out, err = go("clean", ["run", "-q"])
check("program's own output untouched", out == b"5\n" and code == 0)
# only agent mode drops them
code, out, err = go("errors3", ["build"], flags=("--diagnostics-format=json",))
check("feature flags without agent mode keep cargo's closing lines", any("could not compile" in o for o in records(err)[1]))
code, out, err = go("errors3", ["build"], flags=("--agent-mode", "--no-build-summary"))
check("without the summary record the lines stay (nothing would carry the exit code)", any("could not compile" in o for o in records(err)[1]))
code, out, err = go("testmulti", ["test", "--no-fail-fast"], flags=("--agent-mode", "--no-test-report"))
check("without the test report 'test failed, to rerun' stays", any("to rerun pass" in o for o in records(err)[1]))
code, out, err = go("errors3", ["build"], flags=())
check("agent mode off: all of cargo's text, byte for byte plain", b"error: could not compile `errors3`" in err and records(err)[0] == [])
print("FAILED" if fails else "ALL PASS"); sys.exit(1 if fails else 0)
