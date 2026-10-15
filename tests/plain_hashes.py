#!/usr/bin/env python3
"""Agent mode OFF: hashes of exit/stdout/stderr (timings and paths normalised) of the reference jobs for BIN. Compare two binaries:
plain_hashes.py BASE > a; plain_hashes.py NEW > b; cmp a b. Fixtures are copied to a fixed directory so paths are equal."""
import os, shutil, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
from baseline import JOBS
bin_ = sys.argv[1]
base = os.path.expanduser("~/work/tmp/plainhash"); shutil.rmtree(base, ignore_errors=True); os.makedirs(base)
for f in ("clean", "errors3", "warns", "testfail", "testpass", "unresolved", "ownership", "cascade", "docfail"): shutil.copytree(os.path.join(FX, f), os.path.join(base, f))
JOBS = dict(JOBS); JOBS["docfail_test"] = ("docfail", ["test", "-j2", "--offline", "--", "--test-threads=1"])
for n, (f, args) in JOBS.items():
    tgt = os.path.join(base, "tgt-" + n)
    code, out, err = run([bin_, TOOLCHAIN], args, os.path.join(base, f), target=tgt)
    print(f"{n:18} exit {code:>3} stdout {sha(norm(out, tgt))[:16]} stderr {sha(norm(err, tgt))[:16]}")
shutil.rmtree(base, ignore_errors=True)
