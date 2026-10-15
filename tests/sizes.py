#!/usr/bin/env python3
"""Record sizes in agent mode for the reference jobs (checklist item 4 / rule G). Runs the REAL tool, parses the JSON lines.
usage: sizes.py [BIN]            print bytes per job and per record type
       sizes.py --check [BIN]    also fail when a record type or a job grows past its budget (BUDGET_* below)"""
import json, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
J = ["-j2", "--offline"]
# name: (fixture, args, extra env)
JOBS = {
  "errors3_build":    ("errors3",    ["build"] + J, {}),
  "warns_build":      ("warns",      ["build"] + J, {}),
  "warns_clippy":     ("warns",      ["clippy"] + J, {}),
  "cascade_build":    ("cascade",    ["build"] + J, {}),
  "unresolved_build": ("unresolved", ["build"] + J + ["--explain-missing-crate"], {}),
  "ownership_build":  ("ownership",  ["build"] + J + ["--ownership-trace"], {}),
  "warns_fixpreview": ("warns",      ["build"] + J + ["--fix-preview"], {}),
  "testfail_test":    ("testfail",   ["test"] + J + ["--", "--test-threads=1"], {}),
  "docfail_test":     ("docfail",    ["test"] + J + ["--", "--test-threads=1"], {}),
  "testpass_test":    ("testpass",   ["test"] + J + ["--", "--test-threads=1"], {}),
  "panic_report":     ("testfail",   ["test"] + J + ["--panic-report", "--", "--test-threads=1"], {}),
  "clean_build":      ("clean",      ["build"] + J, {}),
  "manywarns_build":  ("manywarns",  ["build"] + J, {}),
  "why_feature":      ("withdep",    ["--explain-feature=serde_json", "--features", "fancy", "--offline"], {}),
  "why_rebuilt":      ("withdep",    ["build"] + J + ["--explain-rebuild=withdep", "--explain-rebuild=serde_json"], {}),
}
# budgets in bytes: per record type (largest single record) and per job (all records)
BUDGET_REC = {"diagnostic": 2000, "test": 800, "test-suite": 300, "summary": 600, "panic-report": 1500, "ownership_trace": 1500,
              "fix-preview": 1500, "missing-crate": 700, "why-feature": 1800, "why-rebuilt": 1000}
BUDGET_JOBS = {"ownership_build": 7000, "manywarns_build": 21000}   # six ownership errors with traces
BUDGET_JOB = 5000   # typical command; the hard cap per command is 20000 (driver.rs HARD_BUDGET)
def measure(bin_, jobs=JOBS):
    tmp, fx = private_fixtures("errors3", "warns", "cascade", "unresolved", "ownership", "testfail", "docfail", "testpass", "clean", "withdep", "manywarns")
    res = {}
    for name, (f, args, env) in jobs.items():
        tgt = tempfile.mkdtemp(prefix="tgt-", dir=tmp)
        code, out, err = run([bin_, TOOLCHAIN], args, fx[f], env=clean_env(RUSTY_AGENT="1", RUSTY_AGENT_DETAIL=os.environ.get("DETAIL", "normal"), **env), target=tgt)
        recs, other = records(err)
        per = {}; mx = {}
        for r in recs:
            n = len(json.dumps(r, separators=(",", ":")))
            per[r["type"]] = per.get(r["type"], 0) + n; mx[r["type"]] = max(mx.get(r["type"], 0), n)
            for k, v in r.items():
                if k in ("ownership_trace",) and isinstance(v, dict): mx[k] = max(mx.get(k, 0), len(json.dumps(v, separators=(",", ":"))))
        res[name] = {"total": sum(per.values()), "by_type": per, "max_record": mx, "records": len(recs), "stderr_bytes": len(err)}
    shutil.rmtree(tmp, ignore_errors=True)
    return res
if __name__ == "__main__":
    a = [x for x in sys.argv[1:] if x != "--check"]; bin_ = a[0] if a else RC
    res = measure(bin_); bad = 0
    for n, r in res.items():
        print(f"{n:18} total {r['total']:>6} records {r['records']:>3}  " + " ".join(f"{t}={b}" for t, b in r["by_type"].items()))
        if "--check" in sys.argv:
            for t, m in r["max_record"].items():
                if t in BUDGET_REC and m > BUDGET_REC[t]: print(f"  FAIL {n}: largest {t} record {m} > {BUDGET_REC[t]}"); bad += 1
            if r["total"] > BUDGET_JOBS.get(n, BUDGET_JOB): print(f"  FAIL {n}: total {r['total']} > {BUDGET_JOBS.get(n, BUDGET_JOB)}"); bad += 1
    sys.exit(1 if bad else 0)
