#!/usr/bin/env python3
"""Feature 9: --explain-rebuild=NAME (why was it rebuilt) and --explain-feature=NAME (why is it on)."""
import os, subprocess, sys, tempfile, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
fails = 0
def check(name, cond, detail=""):
    global fails
    if not cond: fails += 1
    print(("ok   " if cond else "FAIL ") + name + ("" if cond else "  " + str(detail)[:900]))
tmp, fx = private_fixtures("withdep")
w = fx["withdep"]; tgt = tempfile.mkdtemp(dir=tmp)
J = ["-j4", "--offline"]
def build(args=(), env=None, flags=()):
    code, out, err = run([RC, TOOLCHAIN], ["build"] + list(args) + list(flags) + J, w, env=env or clean_env(), target=tgt)
    return code, out, err, records(err)[0], err
def why(recs): return {r["query"]: r for r in recs if r["type"] == "why-rebuilt"}
build()                                   # first build, no flags
def touch_append(path, text): open(os.path.join(w, path), "a").write(text)
time.sleep(0.05)
touch_append("src/main.rs", "// edit\n")
code, out, err, recs, raw = build(flags=("--explain-rebuild=withdep", "--explain-rebuild=serde_json"))
r = why(recs)
u = r["withdep"]["units"][0]
check("edited source: rebuilt, reason is the file", u["verdict"] == "rebuilt" and u["reason_kind"] == "file-changed" and u["details"]["file"] == "src/main.rs", u)
check("cargo's own wording is kept; the log lines that only repeat it or dump Rust Debug text are not (agent-fixes 2026-10-14)", "has changed" in u["reason"] and not any(("stale: changed" in l or "FileTime" in l or "dirty:" in l) for l in u["fingerprint_log"]), u)
check("untouched dependency: fresh", r["serde_json"]["units"][0]["verdict"] == "fresh" and r["serde_json"]["units"][0]["reason_kind"] == "fresh")
text = "\n".join(l for l in raw.decode().splitlines() if not l.startswith("{"))
check("no Fresh/Dirty/Running -v lines and no log lines leaked into the text", not any(w_ in text for w_ in (" Fresh ", " Dirty ", "Running `", "fingerprint:", "DEBUG", "INFO")), text[:400])
check("exit code and normal text intact (Compiling/Finished still shown)", code == 0 and b"Compiling withdep" in raw and b"Finished" in raw)
# dependency chain: edit the path dependency
time.sleep(0.05)
touch_append("inner/src/lib.rs", "// edit\n")
code, out, err, recs, raw = build(flags=("--explain-rebuild=withdep", "--explain-rebuild=inner"))
r = why(recs)
u = r["withdep"]["units"][0]
check("path dependency edited: dependent rebuilt because the dependency was", u["reason_kind"] == "dependency-rebuilt" and u["details"]["dependency"] == "inner", u)
check("caused_by chain ends at the file that changed", u["caused_by"][-1]["package"] == "inner" and u["caused_by"][-1]["reason_kind"] == "file-changed" and u["caused_by"][-1]["details"]["file"] == "inner/src/lib.rs", u["caused_by"])
# features changed: new fingerprint, no Dirty line; the fingerprint files are compared
code, out, err, recs, raw = build(args=["--features", "fancy"], flags=("--explain-rebuild=serde_json",))
u = why(recs)["serde_json"]["units"][0]
check("feature turned on: rebuilt with no earlier fingerprint, and the differing field is named", u["verdict"] == "rebuilt" and u["reason_kind"] == "no-previous-fingerprint" and any(f["field"] == "features" for f in u["fingerprint_diff"]["fields"]), u)
f = [f for f in u["fingerprint_diff"]["fields"] if f["field"] == "features"][0]
check("the new feature list contains preserve_order", "preserve_order" in str(f["new"]) and "preserve_order" not in str(f["old"]), f)
# rustflags
code, out, err, recs, raw = build(env=clean_env(RUSTFLAGS="-C opt-level=1"), flags=("--explain-rebuild=inner",))
u = why(recs)["inner"]["units"][0]
check("RUSTFLAGS changed: rebuilt, field rustflags differs", u["verdict"] == "rebuilt" and any(f["field"] == "rustflags" for f in u["fingerprint_diff"]["fields"]), u)
check("unknown crate: found false", why(build(flags=("--explain-rebuild=nosuchcrate",))[3])["nosuchcrate"]["found"] is False)
# not on by default, and the umbrella does not turn it on
code, out, err, recs, raw = build(flags=("--agent-mode",))
check("not in the umbrella", not any(r["type"] == "why-rebuilt" for r in recs))
# the build output is the same as without the flag
t1 = tempfile.mkdtemp(dir=tmp); t2 = tempfile.mkdtemp(dir=tmp)
run(["cargo", TOOLCHAIN], ["build", "--release"] + J, w, target=t1)
run([RC, TOOLCHAIN], ["build", "--release", "--explain-rebuild=withdep"] + J, w, target=t2)
check("release binary bytes identical with --explain-rebuild", sha(open(t1 + "/release/withdep", "rb").read()) == sha(open(t2 + "/release/withdep", "rb").read()))
# --- why feature
code, out, err = run([RC, TOOLCHAIN], ["--explain-feature=serde_json", "--features", "fancy", "--offline"], w, target=tgt)
recs = records(err)[0]; f = [r for r in recs if r["type"] == "why-feature"]
check("--explain-feature alone: one record, exit 0", code == 0 and len(f) == 1 and f[0]["found"], err[:300])
f = f[0]
fe = {x["feature"]: x for x in f["features"]}
check("features of serde_json that are on are listed", {"preserve_order", "std", "default"} <= set(fe) or {"preserve_order"} <= set(fe), set(fe))
chains = [c["chain"] for c in fe["preserve_order"]["enabled_by"]]
check("chain for preserve_order ends at the command line feature `fancy`", any(c[0].endswith('feature "preserve_order"') and any('withdep feature "fancy" (command-line)' in x for x in c) for c in chains), chains)
check("required_by names the dependents", any(x.startswith("withdep v0.1.0") for x in f["required_by"]), f["required_by"])
check("the tree and cargo's own text are in the record", f["rendered"].startswith("serde_json v") and f["tree"][0]["name"] == "serde_json")
code, out, err = run([RC, TOOLCHAIN], ["tree", "--explain-feature=memchr", "--offline"], w, target=tgt)
check("with another subcommand: that command runs as usual (its stdout), then the record", code == 0 and out.startswith(b"withdep v0.1.0") and any(r["type"] == "why-feature" and r["found"] for r in records(err)[0]), out[:100])
code, out, err = run([RC, TOOLCHAIN], ["--explain-feature=nosuchcrate", "--offline"], w, target=tgt)
r = records(err)[0][0]
check("unknown name: found false with cargo's error", r["found"] is False and "error" in r, r)
print("FAILED" if fails else "ALL PASS"); sys.exit(1 if fails else 0)
