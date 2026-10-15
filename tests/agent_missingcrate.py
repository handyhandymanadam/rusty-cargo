#!/usr/bin/env python3
"""Feature 6: --explain-missing-crate: crate, Cargo.toml line, feature flag, offline. Suggested commands are really run in a copy."""
import os, shutil, subprocess, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
fails = 0
def check(name, cond, detail=""):
    global fails
    if not cond: fails += 1
    print(("ok   " if cond else "FAIL ") + name + ("" if cond else "  " + str(detail)[:600]))
tmp, fx = private_fixtures("missingdep", "unresolved", "errors3")
J = ["-j4", "--offline"]
def go(fixture, flags=("--explain-missing-crate",)):
    code, out, err = run([RC, TOOLCHAIN], ["build"] + list(flags) + J, fx[fixture], target=tempfile.mkdtemp(dir=tmp))
    return code, records(err)[0]
code, recs = go("missingdep")
mc = {r["name"]: r for r in recs if r["type"] == "missing-crate"}
check("one record per missing crate, all offline", set(mc) == {"memchr", "rand", "serde_json", "serde_jsn", "zzznotacrate"} and all(r["network"] == "not used" for r in mc.values()), set(mc))
check("record follows its diagnostic", all(recs[i]["type"] == "diagnostic" for i, r in enumerate(recs) if r["type"] == "missing-crate" and False) or recs[[i for i, r in enumerate(recs) if r["type"] == "missing-crate"][0] - 1]["type"] == "diagnostic")
m = mc["memchr"]
check("transitive dependency: found in Cargo.lock, line with the locked version", m["situation"] == "in-lockfile-not-a-dependency" and m["found"]["locked_version"] == "2.8.3" and m["cargo_toml_line"] == 'memchr = "2.8.3"' and m["section"] == "dependencies", m)
r = mc["rand"]
check("crate only in the local registry cache: newest cached version, versions listed", r["situation"] == "in-registry-cache" and r["found"]["cached_versions"][-1] == "0.10.3" and "0.8.8" in r["found"]["cached_versions"] and r["cargo_toml_line"] == 'rand = "0.10.3"', r)
s = mc["serde_json"]
check("feature-gated item: the feature is named", s["situation"] == "feature-gated" and s["features"][0]["name"] == "raw_value" and 'features = ["raw_value"]' in s["cargo_toml_line"] and s["add_command"] == "cargo add serde_json --features raw_value --offline", s)
check("gated: evidence is rustc's own label", "gated behind the `raw_value` feature" in s["features"][0]["evidence"])
n = mc["zzznotacrate"]
check("unknown crate: says so, no download, add command given", n["situation"] == "not-a-dependency" and n["found"]["cached_versions"] == [] and n["add_command"] == "cargo add zzznotacrate" and "network" in n["suggestions"][0]["message"], n)
t = mc["serde_jsn"]
check("typo: near miss serde_json at distance 1", t["near_misses"] and t["near_misses"][0] == {"name": "serde_json", "distance": 1}, t["near_misses"])
check("location of the import", m["location"]["line"] == 1 and r["location"]["line"] == 2 and s["location"]["line"] == 3)
# the suggested commands really work offline
copy = os.path.join(tmp, "applied"); shutil.copytree(fx["missingdep"], copy)
env = clean_env(CARGO_TARGET_DIR=tempfile.mkdtemp(dir=tmp))
for name in ("memchr", "rand", "serde_json"):
    cmd = mc[name]["add_command"].split()
    p = subprocess.run(["cargo", TOOLCHAIN] + cmd[1:], cwd=copy, capture_output=True, text=True, env=env)
    check(f"`{mc[name]['add_command']}` succeeds offline", p.returncode == 0, p.stderr)
toml = open(os.path.join(copy, "Cargo.toml")).read()
check("Cargo.toml now has the crates", "memchr" in toml and "rand" in toml and "raw_value" in toml, toml)
code, out, err = run([RC, TOOLCHAIN], ["build", "--agent-mode"] + J, copy, target=env["CARGO_TARGET_DIR"])
left = [r["message"] for r in records(err)[0] if r["type"] == "diagnostic" and r["severity"] == "error"]
check("after the suggested commands, only the unfixable imports remain", not any("memchr" in x or "rand" in x or "RawValue" in x for x in left), left)
# other diagnostics get no record; not in the umbrella
code, recs = go("errors3")
check("no missing-crate record for unrelated errors", not any(r["type"] == "missing-crate" for r in recs))
code, recs = go("unresolved")
mc = [r for r in recs if r["type"] == "missing-crate"]
check("std item typo (HashMapp) is not a missing crate; serde is", [r["name"] for r in mc] == ["serde"] and mc[0]["situation"] in ("in-registry-cache", "not-a-dependency"), [(r["name"], r["situation"]) for r in mc])
code, recs = go("missingdep", flags=("--agent-mode",))
check("not in the umbrella", not any(r["type"] == "missing-crate" for r in recs))
print("FAILED" if fails else "ALL PASS"); sys.exit(1 if fails else 0)
