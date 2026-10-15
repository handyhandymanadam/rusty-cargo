#!/usr/bin/env python3
"""Feature 5: --fix-preview is a dry run. Safe diff applied with patch must equal what real `cargo fix` does (in a copy);
nothing is written to the project; unsafe suggestions are only in diff_with_unsafe."""
import os, shutil, subprocess, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
fails = 0
def check(name, cond, detail=""):
    global fails
    if not cond: fails += 1
    print(("ok   " if cond else "FAIL ") + name + ("" if cond else "  " + str(detail)[:500]))
tmp, fx = private_fixtures("warns", "unresolved", "clean")
J = ["-j4", "--offline"]
def go(fixture, cmd="check", flags=("--fix-preview",)):
    code, out, err = run([RC, TOOLCHAIN], [cmd] + list(flags) + J, fx[fixture], target=tempfile.mkdtemp(dir=tmp))
    return code, records(err)[0]
def treesum(p): return {f: sha(open(os.path.join(p, "src", f), "rb").read()) for f in os.listdir(os.path.join(p, "src"))}
before = treesum(fx["warns"])
code, recs = go("warns")
fp = [r for r in recs if r["type"] == "fix-preview"]
check("one fix-preview record, dry_run true, written false", len(fp) == 1 and fp[0]["dry_run"] is True and fp[0]["written"] is False, [r["type"] for r in recs])
fp = fp[0]
check("project files untouched", treesum(fx["warns"]) == before)
check("3 safe edits in one file (import, two unused variables)", len(fp["files"]) == 1 and fp["files"][0]["safe_edits"] == 3 and fp["files"][0]["unsafe_edits"] == 0 and fp["files"][0]["conflicts"] == 0, fp["files"])
check("diff is a unified diff with a/ b/ headers", fp["files"][0]["diff"].startswith("--- a/src/main.rs\n+++ b/src/main.rs\n@@") and "-use std::collections::HashMap;" in fp["files"][0]["diff"] and "+fn unused_arg(_x: i32)" in fp["files"][0]["diff"], fp["files"][0]["diff"])
# compare with the real thing: cargo fix in a copy (and prove it really changed something)
copy1 = os.path.join(tmp, "fixcopy"); shutil.copytree(fx["warns"], copy1)
r = subprocess.run(["cargo", TOOLCHAIN, "fix", "--allow-no-vcs", "--offline", "-j4"], cwd=copy1, capture_output=True, env=clean_env(CARGO_TARGET_DIR=tempfile.mkdtemp(dir=tmp)))
copy2 = os.path.join(tmp, "patchcopy"); shutil.copytree(fx["warns"], copy2)
open(os.path.join(tmp, "safe.diff"), "w").write(fp["files"][0]["diff"])
p = subprocess.run(["patch", "-p1", "-s", "-i", os.path.join(tmp, "safe.diff")], cwd=copy2, capture_output=True, text=True)
check("patch -p1 applies the safe diff", p.returncode == 0, p.stdout + p.stderr)
check("cargo fix really changed the copy", open(os.path.join(copy1, "src/main.rs"), "rb").read() != open(os.path.join(fx["warns"], "src/main.rs"), "rb").read())
check("safe diff applied == cargo fix's result, byte for byte", open(os.path.join(copy1, "src/main.rs"), "rb").read() == open(os.path.join(copy2, "src/main.rs"), "rb").read(), r.stderr[-300:])
check("edits listed with safety, applicability and offsets", all(e["safety"] == "safe" and e["applicability"] == "MachineApplicable" and e["in_diff"] for e in fp["edits"]) and len(fp["edits"]) == 3)
# unsafe suggestions: unresolved import similar name is MaybeIncorrect
code, recs = go("unresolved", flags=("--fix-preview",))
fp = [r for r in recs if r["type"] == "fix-preview"][0]
f = fp["files"][0]
check("MaybeIncorrect suggestion is not in the safe diff but is in diff_with_unsafe", f["diff"] == "" and "+use std::collections::HashMap;" in f["diff_with_unsafe"] and f["unsafe_edits"] >= 1, f)
check("unsafe edits are labelled", any(e["safety"] == "unsafe" and e["applicability"] == "MaybeIncorrect" and e["in_diff_with_unsafe"] and not e["in_diff"] for e in fp["edits"]), fp["edits"])
# clippy suggestions go through the same path
code, recs = go("warns", cmd="clippy")
fp = [r for r in recs if r["type"] == "fix-preview"][0]
check("clippy fixes included (is_empty, clone)", "+    if v.is_empty()" in fp["files"][0]["diff"] and "-    let m = n.clone();" in fp["files"][0]["diff"], fp["files"][0]["diff"])
# no suggestions: record still present, empty
code, recs = go("clean")
fp = [r for r in recs if r["type"] == "fix-preview"]
check("nothing to fix: record with no files", len(fp) == 1 and fp[0]["files"] == [], fp)
code, recs = go("warns", flags=("--agent-mode",))
check("not in the umbrella", not any(r["type"] == "fix-preview" for r in recs))
print("FAILED" if fails else "ALL PASS"); sys.exit(1 if fails else 0)
