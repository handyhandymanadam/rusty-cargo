#!/usr/bin/env python3
"""Feature 4: --ownership-trace on real borrow-checker and lifetime errors; roles, order and locations checked against the source."""
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
fails = 0
def check(name, cond, detail=""):
    global fails
    if not cond: fails += 1
    print(("ok   " if cond else "FAIL ") + name + ("" if cond else "  " + str(detail)[:500]))
tmp, fx = private_fixtures("ownership", "errors3", "clean")
J = ["-j4", "--offline"]
src = open(os.path.join(fx["ownership"], "src/main.rs")).read().splitlines()
def go(fixture, flags):
    code, out, err = run([RC, TOOLCHAIN], ["build"] + flags + J, fx[fixture], target=tempfile.mkdtemp(dir=tmp))
    return code, records(err)[0]
code, recs = go("ownership", ["--agent-mode", "--ownership-trace"])
d = {}
for r in recs:
    if r["type"] == "diagnostic" and r.get("rustc_code"): d.setdefault(r["rustc_code"], []).append(r)
check("every ownership error has a trace", all("ownership_trace" in r for rs in d.values() for r in rs) and sum(len(v) for v in d.values()) == 5, {k: len(v) for k, v in d.items()})
def ev(r): return {e["role"]: e for e in r["ownership_trace"]["events"]}
def text_at(e):
    l = e["location"]; return src[l["line"] - 1][l["column"] - 1:l["end_column"] - 1]
m = d["E0382"][0]; t = m["ownership_trace"]; e = ev(m)
check("E0382 move then use: kind, variable, type", (t["kind"], t["variable"], t["type"]) == ("use-after-move", "s", "String"), t)
check("E0382 order is declared, moved, use_after_move", [x["role"] for x in t["events"]] == ["declared", "moved", "use_after_move"] and [x["order"] for x in t["events"]] == [0, 1, 2])
check("E0382 locations point at the real source text", text_at(e["moved"]) == "s" and e["moved"]["location"]["line"] == 5 and e["use_after_move"]["location"]["line"] == 6 and e["declared"]["location"]["line"] == 4, (e["moved"], e["use_after_move"]))
check("E0382 events carry rustc's own label and are not heuristic", e["moved"]["label"] == "value moved here" and not any(x["heuristic"] for x in t["events"]))
m2 = d["E0382"][1]
check("E0382 into a call: the callee's 'takes ownership' parameter is a related location", "takes_ownership" in ev(m2) and ev(m2)["takes_ownership"]["location"]["line"] == 1, ev(m2).keys())
b = d["E0499"][0]; e = ev(b)
check("E0499 two mutable borrows: first, second, later use in that order", [x["role"] for x in b["ownership_trace"]["events"]] == ["borrowed", "conflicting_borrow", "later_use"] and text_at(e["borrowed"]) == "&mut v" and e["borrowed"]["location"]["line"] == 17 and e["conflicting_borrow"]["location"]["line"] == 18, e)
s = d["E0597"][0]; e = ev(s)
check("E0597 does not live long enough: declared, borrowed, dropped, later use", [x["role"] for x in s["ownership_trace"]["events"]] == ["declared", "borrowed_value", "dropped", "later_use"] and e["dropped"]["location"]["line"] == 28 and e["later_use"]["location"]["line"] == 29, e)
a = d["E0506"][0]; e = ev(a)
check("E0506 assign while borrowed", a["ownership_trace"]["kind"] == "assign-while-borrowed" and e["conflicting_assign"]["location"]["line"] == 35 and e["borrowed"]["location"]["line"] == 34 and e["later_use"]["location"]["line"] == 36, e)
check("complete=true and nothing missing for these", all(r["ownership_trace"]["complete"] and r["ownership_trace"]["missing"] == [] for rs in d.values() for r in rs))
# E0502 and a case where rustc's JSON lacks a relation
code, recs = go("errors3", ["--agent-mode", "--ownership-trace"])
e502 = [r for r in recs if r.get("rustc_code") == "E0502"][0]
check("E0502 conflicting borrow with later use", [x["role"] for x in e502["ownership_trace"]["events"]] == ["borrowed", "conflicting_borrow", "later_use"], e502["ownership_trace"])
check("other diagnostics get no trace", all("ownership_trace" not in r for r in recs if r.get("rustc_code") in ("E0308", "E0425")))
# opt-in only
code, recs = go("ownership", ["--agent-mode"])
check("not in the umbrella: no ownership_trace without the flag", not any("ownership_trace" in r for r in recs))
code, recs = go("ownership", ["--ownership-trace"])
check("works alone (flag implies diagnostic records)", sum("ownership_trace" in r for r in recs) == 5)
print("FAILED" if fails else "ALL PASS"); sys.exit(1 if fails else 0)
