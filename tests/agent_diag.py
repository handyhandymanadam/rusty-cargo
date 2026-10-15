#!/usr/bin/env python3
"""Features 1, 2, 3, 12, 13 (diagnostics, root cause, type fields, clippy, stable mode) on real cargo jobs; JSON parsed here."""
import os, subprocess, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
fails = 0
def check(name, cond, detail=""):
    global fails
    if not cond: fails += 1
    print(("ok   " if cond else "FAIL ") + name + ("" if cond else "  " + str(detail)[:400]))
tmp, fx = private_fixtures("clean", "errors3", "warns", "unresolved", "cascade", "ownership")
J = ["-j4", "--offline"]
def agent(fixture, args, flags=("--agent-mode",), env=None):
    tgt = tempfile.mkdtemp(prefix="tgt-", dir=tmp)
    code, out, err = run([RC, TOOLCHAIN], list(args) + list(flags) + J, fx[fixture], env=env, target=tgt)
    recs, other = records(err)
    return code, out, recs, other, err
def plain(fixture, args):
    tgt = tempfile.mkdtemp(prefix="tgt-", dir=tmp)
    return run(["cargo", TOOLCHAIN], list(args) + J, fx[fixture], target=tgt)
def diags(recs): return [r for r in recs if r["type"] == "diagnostic"]

# --- feature 1: errors3 = type mismatch + unresolved fn + borrow error
code, out, recs, other, raw = agent("errors3", ["build"])
d = diags(recs)
check("errors3: exit code 101 like cargo", code == 101 == plain("errors3", ["build"])[0])
check("errors3: stdout empty, 3 diagnostic records + summary", out == b"" and len(d) == 3 and recs[-1]["type"] == "summary", (len(d), [r["type"] for r in recs]))
by = {r["rustc_code"]: r for r in d}
check("rustc codes kept in rustc_code", set(by) == {"E0308", "E0425", "E0502"}, set(by))
check("R codes set explicitly: E0308 R0004, E0425 R0001, E0502 R0006", (by["E0308"]["code"], by["E0425"]["code"], by["E0502"]["code"]) == ("R0004", "R0001", "R0006"))
for r in d:
    check("record shape " + r["rustc_code"], r["schema"] == 1 and r["severity"] == "error" and isinstance(r["files"], list) and isinstance(r["symbols"], list) and isinstance(r["notes"], list)
          and isinstance(r["suggestions"], list) and isinstance(r["locations"], list) and isinstance(r["fixits"], list) and isinstance(r["rendered"], str) and r["flag"] is None)
e = by["E0425"]
check("symbols from the message", e["symbols"] == ["missing_fn"], e["symbols"])
check("primary location has line/column and label", e["locations"][0]["role"] == "primary" and e["locations"][0]["line"] == 15 and e["locations"][0]["column"] == 5 and e["locations"][0]["label"] == "not found in this scope", e["locations"])
check("rendered is the text rustc printed, once", "error[E0425]: cannot find function `missing_fn`" in e["rendered"] and "not found in this scope" in e["rendered"])
p = plain("errors3", ["build"])[2].decode()
check("rendered text of every record appears verbatim in plain cargo's output", all(r["rendered"].rstrip("\n") in p for r in d))
check("root is the directory the file names are relative to", os.path.exists(os.path.join(e["root"], e["files"][0])))
# --- feature 3: type mismatch fields
tm = by["E0308"]["type_mismatch"]
check("type mismatch: expected i32, found &str, from the label, not heuristic", tm["expected"] == "i32" and tm["found"] == "&str" and tm["source"] == "label" and tm["heuristic"] is False, tm)
check("type mismatch location is the primary span (line 4)", tm["location"]["line"] == 4 and tm["location"]["role"] == "primary")
check("function-defined-here note is kept", any("function defined here" in n for n in by["E0308"]["notes"]), by["E0308"]["notes"])
# borrow error: secondary locations with labels
b = by["E0502"]
check("borrow error: labelled secondary locations", {l["label"] for l in b["locations"]} >= {"mutable borrow occurs here", "immutable borrow occurs here", "immutable borrow later used here"}, b["locations"])
# --- stable mode: sorted by place, deterministic
lines = [r["locations"][0]["line"] for r in d]
check("stable order: by place within the unit", lines == sorted(lines), lines)
c2, o2, recs2, _, raw2 = agent("errors3", ["build"])
def scrub(rs): return [{k: v for k, v in r.items() if k not in ("elapsed_ms", "units", "command", "status_lines_consumed")} for r in rs]
check("stable: two runs give identical records (except clock values)", scrub(recs) == scrub(recs2))
check("no ANSI escapes anywhere", b"\x1b[" not in raw)
check("progress and status lines are consumed in stable mode, summary counts them", b"Compiling" not in raw and recs[-1].get("status_lines_consumed", 0) >= 1, raw[:200])
# --- clippy (feature 12) and fixits
code, out, recs, other, raw = agent("warns", ["clippy"])
d = diags(recs)
flags = sorted(r["flag"] for r in d)
check("clippy: rustc lints and clippy lints in one stream", flags == ["clippy::clone_on_copy", "clippy::len_zero", "unused_imports", "unused_variables", "unused_variables"], flags)
cl = [r for r in d if r["flag"].startswith("clippy::")]
check("clippy lints get R0011, rustc unused lints R0009, rustc_code null", all(r["code"] == "R0011" and r["rustc_code"] is None for r in cl) and all(r["code"] == "R0009" for r in d if r["flag"].startswith("unused")))
src = open(os.path.join(fx["warns"], "src/main.rs"), "rb").read()
ok = True
for r in d:
    for f in r["fixits"]:
        ok &= f["applicability"] == "MachineApplicable" and 0 <= f["start"]["offset"] <= f["end"]["offset"] <= len(src)
check("fixits: applicability and byte offsets inside the file", ok)
lz = [r for r in cl if r["flag"] == "clippy::len_zero"][0]
f = lz["fixits"][0]
check("fixit text and the source it replaces", f["text"] == "v.is_empty()" and src[f["start"]["offset"]:f["end"]["offset"]] == b"v.len() == 0", (f, src[f["start"]["offset"]:f["end"]["offset"]]))
check("suggestion points at its fixits", lz["suggestions"][0]["fixits"] == [0] and lz["suggestions"][0]["message"] == "using `is_empty` is clearer and more explicit", lz["suggestions"])
check("warnings exit 0, summary lists lints", code == 0 and recs[-1]["diagnostics"]["by_lint"]["unused_variables"] == 2 and recs[-1]["diagnostics"]["warnings"] == 5, recs[-1])
# --- unresolved import
code, out, recs, other, raw = agent("unresolved", ["build"])
d = diags(recs)
check("unresolved imports: E0432 as R0003", len(d) == 2 and {r["code"] for r in d} == {"R0003"} and {r["rustc_code"] for r in d} == {"E0432"}, [(r["code"], r["message"]) for r in d])
sim = [r for r in d if "HashMapp" in r["message"]][0]
check("similar-name suggestion became a fixit", any(f["text"] == "HashMap" for f in sim["fixits"]), sim["fixits"])
# --- feature 2: root cause
code, out, recs, other, raw = agent("cascade", ["build"])
d = diags(recs)
check("root cause: 5 errors become 2 leaders", len(d) == 2 and [r["suppressed"] for r in d] == [2, 1], [(r["message"], r.get("suppressed")) for r in d])
lead = d[0]
check("follow_on lists code, message, place, rule and the full rendered text", len(lead["follow_on"]) == 2 and all(f["rule"] == "same-unresolved-name" and f["rendered"] and f["location"]["line"] for f in lead["follow_on"]) and lead["follow_on"][0]["code"] == "R0001", lead.get("follow_on"))
check("summary counts all 5 errors even when folded", recs[-1]["diagnostics"]["errors"] == 5 and recs[-1]["diagnostics"]["by_rustc_code"] == {"E0425": 5}, recs[-1]["diagnostics"])
code, out, recs, other, raw = agent("cascade", ["build"], flags=("--agent-mode", "--no-root-cause"))
check("--no-root-cause writes all 5", len(diags(recs)) == 5 and all(r["suppressed"] if "suppressed" in r else True for r in diags(recs)))
# --- each feature alone, without the umbrella
code, out, recs, other, raw = agent("errors3", ["build"], flags=("--diagnostics-format=json",))
check("--diagnostics-format=json alone: records, no summary, cargo's text still passes through", len(diags(recs)) == 3 and not any(r["type"] == "summary" for r in recs) and any("Compiling" in o for o in other) and any("could not compile" in o for o in other), other)
check("alone: streamed in cargo's order (not sorted by place) is allowed, still all 3", len(diags(recs)) == 3)
code, out, recs, other, raw = agent("cascade", ["build"], flags=("--root-cause",))
check("--root-cause alone", len(diags(recs)) == 2)
code, out, recs, other, raw = agent("errors3", ["build"], flags=("--build-summary",))
check("--build-summary alone: only the summary record, cargo's text untouched", [r["type"] for r in recs] == ["summary"] and recs[0]["exit"] == 101 and b"error[E0308]: mismatched types" in raw, raw[:300])
tgtp = tempfile.mkdtemp(dir=tmp); tgta = tempfile.mkdtemp(dir=tmp)
pe = norm(run(["cargo", TOOLCHAIN], ["build"] + J, fx["errors3"], target=tgtp)[2], tgtp)
ae = norm(run([RC, TOOLCHAIN], ["build", "--build-summary"] + J, fx["errors3"], target=tgta)[2], tgta)
ae = b"\n".join(l for l in ae.split(b"\n") if not l.startswith(b"{"))
check("--build-summary alone: stderr text equals plain cargo's byte for byte (summary line removed)", pe == ae, (pe[-200:], ae[-200:]))
# --- opt-in only: agent mode off gives no records, and the flag is stripped
code, out, recs, other, raw = agent("errors3", ["build"], flags=())
check("no flag, no records", recs == [])
code, out, recs, other, raw = agent("errors3", ["build"], flags=(), env=clean_env(RUSTY_AGENT="1"))
check("RUSTY_AGENT=1 equals --agent-mode", len(diags(recs)) == 3)
code, out, recs, other, raw = agent("errors3", ["build"], flags=("--no-agent-mode",), env=clean_env(RUSTY_AGENT="1"))
check("--no-agent-mode beats RUSTY_AGENT=1", recs == [])
# --- colour and locale do not leak into records
code, out, recs, other, raw = agent("errors3", ["build"], env=clean_env(CARGO_TERM_COLOR="always"))
check("CARGO_TERM_COLOR=always: still no ANSI in agent mode", b"\x1b[" not in raw and len(diags(recs)) == 3)
c3, o3, recs3, _, raw3 = agent("errors3", ["build"], env=clean_env(LC_ALL="de_DE.UTF-8", LANG="de_DE.UTF-8"))
check("another locale gives the same records", scrub(recs3) == scrub(recs))
# --- artifacts byte-identical in and out of agent mode
t1 = tempfile.mkdtemp(dir=tmp); t2 = tempfile.mkdtemp(dir=tmp)
run(["cargo", TOOLCHAIN], ["build", "--release"] + J, fx["clean"], target=t1)
run([RC, TOOLCHAIN], ["build", "--release", "--agent-mode"] + J, fx["clean"], target=t2)
check("release binary bytes identical in agent mode", sha(open(t1 + "/release/clean", "rb").read()) == sha(open(t2 + "/release/clean", "rb").read()))
print("FAILED" if fails else "ALL PASS"); sys.exit(1 if fails else 0)
