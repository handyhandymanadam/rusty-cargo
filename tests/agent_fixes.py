#!/usr/bin/env python3
"""Agent-fixes round (2026-10-14): stable mode, doc-test location, --explain-feature with a feature name, one panic backtrace,
fresh-unit and why-rebuilt tidy-ups, shared noise. Real tool, real cargo jobs, JSON parsed in python."""
import json, os, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
fails = 0
def check(name, cond, detail=""):
    global fails
    if not cond: fails += 1
    print(("ok   " if cond else "FAIL ") + name + ("" if cond else "  " + str(detail)[:900]))
tmp, fx = private_fixtures("docfail", "testfail", "withdep", "panicker", "errors3", "warns", "clean")
J = ["-j2", "--offline"]
def go(fixture, args, env=None, name=None):
    e = clean_env(RUSTY_AGENT="1", **(env or {}))
    code, out, err = run([RC, TOOLCHAIN], args, fx[fixture], env=e, target=tempfile.mkdtemp(dir=tmp))
    recs, other = records(err)
    return code, out, err, recs, other
def of(recs, t): return [r for r in recs if r["type"] == t]

# ---- 1. stable mode ----
code, out, err, recs, _ = go("docfail", ["test"] + J)
code2, out2, err2, recs2, _ = go("testfail", ["test"] + J)
txt = err.decode() + err2.decode()
import re
check("1: no 'thread x (pid)' anywhere", not re.search(r"thread '[^']*' \(\d+\)", txt))
check("1: no RUST_BACKTRACE note line", "RUST_BACKTRACE" not in txt)
check("1: no random doc-test temp directory", "rustdoctest" not in txt and "/tmp/" not in txt.replace(tmp, ""))
check("1: no clock values in summary or suites", all("elapsed_ms" not in r and "finished_in_s" not in r for r in recs + recs2) and all("ms" not in u for r in of(recs, "summary") for u in r["units"]["times"]))
check("1: test binary path has neither directory nor hash", all(re.match(r"[A-Za-z0-9_]+" + r"\Z", r["binary"]["path"]) for r in of(recs, "test") + of(recs, "test-suite") if r["binary"]["path"]), [r["binary"] for r in of(recs, "test")][:2])
def canon(rs, root):
    return [json.loads(json.dumps(r).replace(root, "ROOT")) for r in rs]
b = tempfile.mkdtemp(dir=tmp); os.rmdir(b)
import shutil; shutil.copytree(fx["docfail"], b + "x"); 
_, _, _, recs_b, _ = (lambda c, o, e: (c, o, e, records(e)[0], None))(*run([RC, TOOLCHAIN], ["test"] + J, b + "x", env=clean_env(RUSTY_AGENT="1"), target=tempfile.mkdtemp(dir=tmp)))
check("1: the same job in another directory gives the same records", canon(recs, fx["docfail"]) == canon(recs_b, b + "x"), "")

# ---- 2. doc test location ----
d = [r for r in of(recs, "diagnostic") if r["code"] == "R0400"]
by = {r["test"]: r for r in d}
triple = next(r for t, r in by.items() if "triple" in t)
src = open(fx["docfail"] + "/src/lib.rs").read().split("\n")
tl = next(r["locations"][0]["line"] for r in [triple])
check("2: assertion failing in the doc test: primary location is the code block start (the ``` line)", triple["locations"][0]["role"] == "primary" and src[tl - 1].strip() == "/// ```" and "triple" in src[tl + 0] + src[tl + 1], triple["locations"])
caller = next(r for t, r in by.items() if "caller" in t)
prim = [l for l in caller["locations"] if l["role"] == "primary"][0]; sec = [l for l in caller["locations"] if l["role"] == "secondary"]
check("2: panic inside library code: block start is primary, the panic site secondary", src[prim["line"] - 1].strip() == "/// ```" and "boom" in src[prim["line"]] and sec and sec[0]["line"] == 3, caller["locations"])

# ---- 3. --explain-feature=NAME with a feature name ----
def feat(name, extra=()):
    code, out, err, recs, _ = go("withdep", ["--explain-feature=" + name, "--offline"] + list(extra))
    return code, of(recs, "why-feature")
code, f = feat("preserve_order", ["--features", "fancy"])
r = f[0] if f else {}
check("3: a feature name: found, kind feature, enabled, one record", code == 0 and len(f) == 1 and r.get("found") and r.get("kind") == "feature" and r.get("enabled") is True, r)
chains = [c["chain"] for p in r.get("packages", []) if p["package"].startswith("serde_json v") for c in p["enabled_by"]]
check("3: the package that has the feature, and the chain up to the command-line feature `fancy`", chains and any('withdep feature "fancy" (command-line)' in x for c in chains for x in c), r)
code, f = feat("preserve_order")
check("3: feature defined but not switched on: found true, enabled false, package listed in defined_but_off", f[0]["found"] and f[0]["enabled"] is False and any(x.startswith("serde_json v") for x in f[0]["defined_but_off"]), f[0])
code, f = feat("fancyy")
check("3: unknown name: found false, hint says package or feature, close names offer `fancy`", f[0]["found"] is False and "package" in f[0]["hint"] and "feature" in f[0]["hint"] and "fancy" in f[0]["close_names"], f[0])
code, f = feat("serde_jsn")
check("3: a misspelt package: close names offer serde_json", "serde_json" in f[0]["close_names"], f[0])
code, f = feat("serde_json", ["--features", "fancy"])
check("3: a package name still gives the package record", f[0]["found"] and f[0].get("kind") == "package", f[0].keys())

# ---- 4. --panic-report: the backtrace once ----
code, out, err, recs, _ = go("panicker", ["test", "--panic-report"] + J + ["--", "--test-threads=1"])
pr = of(recs, "panic-report"); tests = [r for r in of(recs, "test") if r["status"] == "failed"]; dg = [r for r in of(recs, "diagnostic") if r["code"] == "R0400"]
allj = "\n".join(json.dumps(r) for r in recs)
check("4: the std backtrace frames appear in exactly one record (the panic-report)", len(pr) == 1 and allj.count("rust_begin_unwind") == 1 and "rust_begin_unwind" in pr[0]["rendered"], allj.count("rust_begin_unwind"))
check("4: the test record and the R0400 diagnostic say where the backtrace is, by id", tests and dg and tests[0].get("backtrace_in") == pr[0]["id"] == dg[0].get("backtrace_in"), (tests[:1], pr[0].get("id")))
check("4: the panic message and place stay in the test record's text", "panicked at" in tests[0]["rendered"] and "stack backtrace" not in tests[0]["rendered"] and "stack backtrace" not in dg[0]["rendered"], tests[0]["rendered"])

# ---- 5. summary units and why-rebuilt ----
code, out, err, recs, _ = go("errors3", ["build"] + J)
sm = of(recs, "summary")[0]
check("5: the crate that failed to compile is in units.times, marked failed", [t for t in sm["units"]["times"] if t["package"] == "errors3" and t.get("failed") is True], sm["units"])
tg = tempfile.mkdtemp(dir=tmp)
run([RC, TOOLCHAIN], ["build"] + J, fx["withdep"], env=clean_env(), target=tg)   # first build: everything compiled
code, out, err = run([RC, TOOLCHAIN], ["build", "--no-stable-diagnostics"] + J, fx["withdep"], env=clean_env(RUSTY_AGENT="1"), target=tg)   # second: all fresh
recs = records(err)[0]
times = of(recs, "summary")[0]["units"]["times"]
check("5: fresh units have no ms: null (the key is left out), compiled ones keep ms", times and all("ms" not in t for t in times if t["fresh"]) and not any(t.get("ms", 0) is None for t in times), times[:3])
# why-rebuilt: touch a source file and rebuild
tg = tempfile.mkdtemp(dir=tmp)
run([RC, TOOLCHAIN], ["build"] + J, fx["withdep"], env=clean_env(), target=tg)
import time; time.sleep(1.1); os.utime(fx["withdep"] + "/src/main.rs")
code, out, err = run([RC, TOOLCHAIN], ["build"] + J + ["--explain-rebuild=withdep", "--explain-rebuild=serde_json"], fx["withdep"], env=clean_env(RUSTY_AGENT="1"), target=tg)
wr = {r["query"]: r for r in of(records(err)[0], "why-rebuilt")}
u = wr["withdep"]["units"][0]
blob = json.dumps(wr)
check("5: why-rebuilt: no Rust Debug dumps (FileTime, StaleItem, FsStatusOutdated), no clock values", "FileTime" not in blob and "StaleItem" not in blob and "FsStatusOutdated" not in blob and not re.search(r"\d{9,}", blob), blob[:600])
check("5: why-rebuilt: the reason is still plain words and the changed file is named", u["reason"] == "the file `src/main.rs` has changed" and u["details"]["file"] == "src/main.rs", u)
print("FAILED" if fails else "ALL PASS"); sys.exit(1 if fails else 0)
