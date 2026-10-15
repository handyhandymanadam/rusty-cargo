#!/usr/bin/env python3
"""Record the baseline: fixed real cargo jobs on the fixture workspace, exit code, stdout/stderr sha256, timing median of N.
usage: baseline.py "cargo +1.98.1" OUT.json [runs]   (each run builds into a fresh target dir = clean build)"""
import json, os, shutil, statistics, subprocess, sys, tempfile, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
JOBS = {
  "clean_build":      ("clean",      ["build", "-j4", "--offline"]),
  "errors3_build":    ("errors3",    ["build", "-j4", "--offline"]),
  "warns_build":      ("warns",      ["build", "-j4", "--offline"]),
  "warns_clippy":     ("warns",      ["clippy", "-j4", "--offline"]),
  "testfail_test":    ("testfail",   ["test", "-j4", "--offline", "--", "--test-threads=1"]),
  "testpass_test":    ("testpass",   ["test", "-j4", "--offline", "--", "--test-threads=1"]),
  "unresolved_build": ("unresolved", ["build", "-j4", "--offline"]),
  "ownership_build":  ("ownership",  ["build", "-j4", "--offline"]),
  "cascade_build":    ("cascade",    ["build", "-j4", "--offline"]),
  "metadata":         ("clean",      ["metadata", "--format-version=1", "--offline", "--no-deps"]),
  "version":          ("clean",      ["--version", "--verbose"]),
}
def one(comp, name, runs, tmp):
    fx, args = JOBS[name]; cwd = os.path.join(tmp, fx)
    times, res = [], None
    for _ in range(runs):
        tgt = tempfile.mkdtemp(prefix="tgt-", dir=tmp)
        t = time.perf_counter(); code, out, err = run(comp, args, cwd, target=tgt); times.append((time.perf_counter() - t) * 1000)
        shutil.rmtree(tgt, ignore_errors=True)
        r = (code, sha(norm(out, tgt)), sha(norm(err, tgt)), len(err))
        if res and res[:3] != r[:3]: print("UNSTABLE", name, file=sys.stderr)
        res = r
    return {"exit": res[0], "stdout_sha256": res[1], "stderr_sha256": res[2], "stderr_bytes": res[3], "median_ms": round(statistics.median(times), 1)}
if __name__ == "__main__":
    comp = sys.argv[1].split(); outf = sys.argv[2]; runs = int(sys.argv[3]) if len(sys.argv) > 3 else 12
    tmp, _ = private_fixtures("clean", "errors3", "warns", "testfail", "testpass", "unresolved", "ownership", "cascade")
    data = {n: one(comp, n, runs, tmp) for n in JOBS}
    ver = subprocess.run(comp + ["--version"], capture_output=True, text=True).stdout.strip()
    rv = subprocess.run(["rustc", "+1.98.1", "--version"], capture_output=True, text=True).stdout.strip()
    json.dump({"command": comp, "runs": runs, "cargo": ver, "rustc": rv, "jobs": data}, open(outf, "w"), indent=1)
    for n, d in data.items(): print(f"{n:18} exit {d['exit']:>3} stdout {d['stdout_sha256'][:10]} stderr {d['stderr_sha256'][:10]} {d['median_ms']:>8} ms")
    shutil.rmtree(tmp, ignore_errors=True)
