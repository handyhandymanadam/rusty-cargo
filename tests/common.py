"""Shared helpers for the rusty-cargo tests: fixtures are copied to a private temp dir per test (tests run in parallel),
every run gets its own CARGO_TARGET_DIR, outputs are normalised (target dir, timings) before comparing or hashing."""
import hashlib, json, os, re, shutil, subprocess, sys, tempfile, time
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FX = os.path.join(ROOT, "tests/fixtures")
RC = os.environ.get("RUSTY_CARGO_BIN") or os.path.join(ROOT, "target/release/rusty-cargo")
TOOLCHAIN = "+1.98.1"
def clean_env(**extra):
    e = {k: v for k, v in os.environ.items() if k not in ("RUSTY_AGENT", "RUSTY_CARGO_REAL", "CARGO_TARGET_DIR")}
    e.update(extra); return e
def private_fixtures(*names):
    """copy fixtures to a new temp dir; returns (tmp, {name: path})"""
    tmp = tempfile.mkdtemp(prefix="rc-test-")
    out = {}
    for n in names:
        dst = os.path.join(tmp, n); shutil.copytree(os.path.join(FX, n), dst); out[n] = dst
    return tmp, out
def norm(b, target=None):
    if target: b = b.replace(target.encode(), b"TARGET")
    b = re.sub(rb"\b\d+\.\d+s\b", b"Ns", b)
    b = re.sub(rb"/tmp/rc-test-[A-Za-z0-9_]+", b"/TMP", b)
    b = re.sub(rb"\bin \d+s\b", b"in Ns", b)             # "Finished ... in 0.11s", "finished in 0.00s"
    b = re.sub(rb"(thread '[^']*') \(\d+\)", rb"\1 (TID)", b)           # libtest thread ids
    b = re.sub(rb"Finished in \d+(\.\d+)?s", b"Finished in Ns", b)
    b = re.sub(rb"rustdoctest[A-Za-z0-9]+", b"rustdoctestXXXX", b)
    b = re.sub(rb"(?m)^ *Blocking waiting for file lock[^\n]*\n", b"", b)   # appears when parallel tests share the cargo cache lock
    return b
def sha(b): return hashlib.sha256(b).hexdigest()
def run(exe, args, cwd, env=None, target=None, inp=None, timeout=300):
    """run exe+args; returns (exit, stdout, stderr) raw bytes"""
    e = dict(env or clean_env())
    e.setdefault("RUSTY_AGENT_DETAIL", "full")   # the older scripts check every field; `full` is the complete record. agent_detail.py tests normal and brief.
    if target: e["CARGO_TARGET_DIR"] = target
    p = subprocess.run(exe + args, cwd=cwd, capture_output=True, env=e, input=inp, timeout=timeout)
    return p.returncode, p.stdout, p.stderr
def records(stderr):
    """JSON records from stderr: list of dict; non-JSON lines are returned separately"""
    recs, other = [], []
    for ln in stderr.decode(errors="replace").splitlines():
        if ln.startswith("{"):
            try:
                d = json.loads(ln)
                if isinstance(d, dict) and d.get("schema") == 1: recs.append(d); continue
            except ValueError: pass
        other.append(ln)
    return recs, other
def tree_hash(path):
    """hash of all files (names + bytes) under path, skipping fingerprint/incremental dirs that hold timestamps"""
    h = hashlib.sha256()
    for dp, dn, fn in os.walk(path):
        dn.sort()
        for f in sorted(fn):
            p = os.path.join(dp, f); h.update(os.path.relpath(p, path).encode()); h.update(open(p, "rb").read())
    return h.hexdigest()
