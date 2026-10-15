#!/usr/bin/env python3
"""Token-budget rules A-G for rusty-cargo: detail levels, no empty fields, no repeated facts, cheap success, size budget. Real tool, JSON parsed in python."""
import json, os, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
import sizes
fails = 0
def check(name, cond, detail=""):
    global fails
    if not cond: fails += 1
    print(("ok   " if cond else "FAIL ") + name + ("" if cond else "  " + str(detail)[:900]))
# the fixtures below are copied to a private directory per run
tmp, fx = private_fixtures("errors3", "warns", "ownership", "testfail", "testpass", "docfail", "withdep", "clean", "manywarns", "cascade")
J = ["-j2", "--offline"]
def go(fixture, args, detail="normal", env=None):
    e = clean_env(RUSTY_AGENT="1", RUSTY_AGENT_DETAIL=detail, **(env or {}))
    code, out, err = run([RC, TOOLCHAIN], args, fx[fixture], env=e, target=tempfile.mkdtemp(dir=tmp))
    return code, out, err, records(err)[0]
def of(recs, t): return [r for r in recs if r["type"] == t]
def empties(v, path=""):
    """paths of null, "", [] and {} members (key `text` may be empty: it is a deletion)"""
    bad = []
    if isinstance(v, dict):
        for k, x in v.items():
            if k != "text" and (x is None or x == "" or x == [] or x == {}): bad.append(path + "/" + k)
            bad += empties(x, path + "/" + k)
    elif isinstance(v, list):
        for i, x in enumerate(v): bad += empties(x, path + f"[{i}]")
    return bad
JOBS = [("errors3", ["build"] + J), ("warns", ["clippy"] + J), ("ownership", ["build", "--ownership-trace"] + J), ("testfail", ["test", "--panic-report"] + J + ["--", "--test-threads=1"]),
        ("docfail", ["test"] + J), ("cascade", ["build"] + J), ("warns", ["build", "--fix-preview"] + J)]
# A: normal has no rendered and no empty fields
for fixture, args in JOBS:
    code, out, err, recs = go(fixture, args)
    check(f"A normal {fixture} {args[0]}: no `rendered` anywhere, no empty or null field", recs and not any("rendered" in json.dumps(r) and '"rendered"' in json.dumps(r) for r in recs) and not [e for r in recs for e in empties(r)], [e for r in recs for e in empties(r)][:8])
# A: full keeps everything: rendered and the empty fields as before; key order schema, type, code, severity, message
code, out, err, recs = go("errors3", ["build"] + J, "full")
d = of(recs, "diagnostic")[0]
check("A full: rendered, files, symbols, null fields all present", all(k in d for k in ("rendered", "files", "symbols", "flag", "root", "suppressed")) and "expected_kind" in d["type_mismatch"] if "type_mismatch" in d else True, list(d))
check("C key order: schema, type, code, severity, message first (full and normal)", list(d)[:5] == ["schema", "type", "code", "severity", "message"] and all(list(r)[:2] == ["schema", "type"] for r in recs), [list(r)[:3] for r in recs])
code, out, err, recs = go("errors3", ["build"] + J)
d = [r for r in of(recs, "diagnostic") if r.get("type_mismatch")][0]
check("6 type_mismatch at normal: no expected_kind/found_kind nulls, no repeated location, label not repeated", set(d["type_mismatch"]) == {"expected", "found"} and "label" not in d["locations"][0], d)
# brief
code, out, err, rb = go("errors3", ["build"] + J, "brief")
code, out, err, rn = go("errors3", ["build"] + J, "normal")
check("A brief is smaller than normal; brief diagnostic has code, message, one location, counts", len(json.dumps(rb)) < len(json.dumps(rn)) and all(k in of(rb, "diagnostic")[0] for k in ("code", "message", "location")) and "locations" not in of(rb, "diagnostic")[0], of(rb, "diagnostic")[0])
code, out, err, rb = go("warns", ["build"] + J, "brief")
check("A brief: one fix with its edit", any(r.get("fix", {}).get("edits") for r in of(rb, "diagnostic")), of(rb, "diagnostic")[:1])
# flag and env
e = clean_env(RUSTY_AGENT="1", RUSTY_AGENT_DETAIL="full")
c1, o1, e1 = run([RC, TOOLCHAIN], ["build", "--agent-detail=brief"] + J, fx["errors3"], env=e, target=tempfile.mkdtemp(dir=tmp))
check("A --agent-detail=brief wins over RUSTY_AGENT_DETAIL=full", b'"rendered"' not in e1 and b'"location":' in e1, e1[:300])
c1, o1, e1 = run([RC, TOOLCHAIN], ["build"] + J, fx["errors3"], env=clean_env(RUSTY_AGENT="1", RUSTY_AGENT_DETAIL="bogus"), target=tempfile.mkdtemp(dir=tmp))
check("A unknown detail value changes nothing (normal)", b'"rendered"' not in e1 and b'"locations"' in e1)
# B: ownership events point into locations
code, out, err, recs = go("ownership", ["build", "--ownership-trace"] + J)
ok = True; why = ""
for r in of(recs, "diagnostic"):
    for ev in r.get("ownership_trace", {}).get("events", []):
        if "loc" in ev:
            l = r["locations"][ev["loc"]]
            if ev.get("label") is not None and ev.get("label") == l.get("label"): ok = False; why = (ev, l)
        elif "location" in ev: pass
check("B ownership events refer to `locations` by index and do not repeat its labels", ok and any("loc" in ev for r in of(recs, "diagnostic") for ev in r.get("ownership_trace", {}).get("events", [])), why)
code, out, err, recs = go("ownership", ["build", "--ownership-trace"] + J, "full")
check("B full: events keep their own location and label as before", all("location" in ev and "label" in ev for r in of(recs, "diagnostic") for ev in r.get("ownership_trace", {}).get("events", [])))
# B: panic-report. one backtrace; test record + diagnostic do not repeat the panic
code, out, err, recs = go("testfail", ["test", "--panic-report"] + J + ["--", "--test-threads=1"])
check("B failing test: the panic message is not in three records", sum(json.dumps(r).count("assertion `left == right` failed") for r in recs) <= 2, [json.dumps(r)[:200] for r in recs])
# E: success is cheap
code, out, err, recs = go("clean", ["build"] + J)
check("E clean build: the summary line only, small, no command echo, no unit list", len(recs) == 1 and recs[0]["type"] == "summary" and len(json.dumps(recs[0])) < 120 and "command" not in recs[0] and "times" not in recs[0].get("units", {}), recs)
code, out, err, recs = go("testpass", ["test"] + J)
check("E passing tests: suite counts only, no record per passing test", not of(recs, "test") and of(recs, "test-suite"), [r["type"] for r in recs])
code, out, err, recs = go("testpass", ["test"] + J, "full")
check("E full: every test has its record", len(of(recs, "test")) >= 2)
code, out, err, recs = go("errors3", ["build"] + J)
check("E summary: no command echo or status counters at normal; full has them", "command" not in of(recs, "summary")[0])
code, out, err, recs = go("errors3", ["build"] + J, "full")
check("E summary full: command echoed", "command" in of(recs, "summary")[0])
# why-feature once
code, out, err, recs = go("withdep", ["--explain-feature=serde_json", "--features", "fancy", "--offline"])
w = of(recs, "why-feature")[0]
check("6 why-feature at normal: the features once, no tree and no rendered text", "features" in w and "tree" not in w and "rendered" not in w, list(w))
code, out, err, recs = go("withdep", ["--explain-feature=serde_json", "--features", "fancy", "--offline"], "full")
check("6 why-feature at full: features, tree and rendered", all(k in of(recs, "why-feature")[0] for k in ("features", "tree", "rendered")))
# fix preview: edits once
code, out, err, recs = go("warns", ["build", "--fix-preview"] + J)
fp = of(recs, "fix-preview")[0]
check("6 fix-preview at normal: the diff carries the edits; no edits list for edits already in the diff; diagnostics without fixits", fp["files"][0]["diff"] and "edits" not in fp and all("fixits" not in d for d in of(recs, "diagnostic")), list(fp))
# G: budget
code, out, err, recs = go("manywarns", ["build"] + J)
tot = sum(len(json.dumps(r, separators=(",", ":"))) for r in recs); sm = of(recs, "summary")[0]
check("G 150 warnings: stderr records stay under 20 KB and the summary says what was cut", tot <= 20500 and sm.get("records_omitted", {}).get("by_type", {}).get("diagnostic", 0) > 100 and "more" in sm["records_omitted"], (tot, sm))
check("G the summary still counts every warning", sm["diagnostics"]["warnings"] == 150, sm["diagnostics"])
code, out, err, recs = go("manywarns", ["build"] + J, "full")
check("G full has no cap: all 150 diagnostics", len(of(recs, "diagnostic")) == 150)
of_file = os.path.join(tmp, "records.jsonl")
code, out, err, recs = go("manywarns", ["build"] + J, "normal", env={"RUSTY_AGENT_OUT": of_file})
filed = [json.loads(l) for l in open(of_file)]
check("G RUSTY_AGENT_OUT file keeps every record that stderr dropped", len([r for r in filed if r["type"] == "diagnostic"]) == 150 and "RUSTY_AGENT_OUT" in of(recs, "summary")[0]["records_omitted"]["more"])
# size budget per record type (checklist item 4)
res = sizes.measure(RC)
for n, r in res.items():
    for t, m in r["max_record"].items():
        b = sizes.BUDGET_REC.get(t)
        if b: check(f"G size: {n}: largest {t} record {m} bytes <= {b}", m <= b)
    lim = sizes.BUDGET_JOBS.get(n, sizes.BUDGET_JOB)
    check(f"G size: {n}: whole command {r['total']} bytes <= {lim}", r["total"] <= lim, r["by_type"])
print("FAILED" if fails else "ALL PASS"); sys.exit(1 if fails else 0)
