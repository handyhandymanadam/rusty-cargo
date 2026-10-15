#!/usr/bin/env python3
"""Checklist item 2: with RUSTY_AGENT unset and no agent flag, rusty-cargo equals plain cargo: exit code, stdout and stderr
bytes, arguments and environment cargo sees, build artifacts, drop-in use through PATH (symlink named cargo), overhead < 2 ms."""
import os, statistics, subprocess, sys, tempfile, stat, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
CARGO = ["cargo", TOOLCHAIN]
fails = 0
def check(name, a, b):
    global fails
    ok = a == b
    if not ok: fails += 1
    print(("ok   " if ok else "FAIL ") + name)
    if not ok: print("  cargo:", repr(a)[:300], "\n  rusty:", repr(b)[:300])
tmp, fx = private_fixtures("clean", "errors3", "warns", "testfail", "testpass", "unresolved", "ownership")
links = os.path.join(tmp, "links"); os.makedirs(links); os.symlink(RC, os.path.join(links, "cargo"))
def both(name, fixture, args, cargo_args=None, env=None):
    cwd = fx[fixture]
    r = []
    for exe in (CARGO, [RC, TOOLCHAIN]):
        tgt = tempfile.mkdtemp(prefix="tgt-", dir=tmp)
        code, out, err = run(exe, args, cwd, env=env, target=tgt)
        r.append((code, norm(out, tgt), norm(err, tgt)))
    check(name, r[0], r[1])
J = ["-j4", "--offline"]
both("clean build", "clean", ["build"] + J)
both("errors3 (type mismatch, unresolved fn, borrow)", "errors3", ["build"] + J)
both("ownership errors", "ownership", ["build"] + J)
both("warnings", "warns", ["build"] + J)
both("clippy lints", "warns", ["clippy"] + J)
both("unresolved import", "unresolved", ["build"] + J)
both("failing tests", "testfail", ["test"] + J + ["--", "--test-threads=1"])
both("passing tests + doctests", "testpass", ["test"] + J + ["--", "--test-threads=1"])
both("check", "clean", ["check"] + J)
both("run (program stdout)", "clean", ["run", "-q"] + J)
both("metadata", "clean", ["metadata", "--format-version=1", "--offline", "--no-deps"])
both("version", "clean", ["--version", "--verbose"])
both("help", "clean", ["build", "--help"])
both("fmt --check", "clean", ["fmt", "--check"])
both("tree", "clean", ["tree", "--offline"])
both("unknown subcommand", "clean", ["no-such-subcommand"])
both("bad flag", "clean", ["build", "--no-such-flag"])
both("explain-ish unknown flag spelled like ours", "clean", ["build", "--agent"] + J)
both("flags after -- are not ours", "testpass", ["test"] + J + ["--", "--test-threads=1", "--agent-mode"])
both("color always", "errors3", ["build", "--color", "always"] + J)
# exit code of a program killed or failing is passed on
both("run args after --", "clean", ["run", "-q"] + J + ["--", "--agent-mode"])
# arguments and environment seen by cargo: a recording fake cargo (compiled C, so argv[0] is visible)
rec = os.path.join(tmp, "fake-cargo"); out = os.path.join(tmp, "seen.txt")
subprocess.run(["clang", "-O1", "-o", rec, os.path.join(ROOT, "tests/helpers/fake_cargo.c")], check=True)
def seen(exe, args, extra):
    e = clean_env(SEEN=out, FOO="bar baz", **extra)
    p = subprocess.run(exe + args, env=e, cwd=tmp); lines = open(out, "rb").read().split(b"\n")
    return p.returncode, [l for l in lines if not l.startswith((b"env=_=", b"env=RUSTY_CARGO_REAL="))]
tricky = ["+nightly", "build", "--release", "-p", "a b", "--features=x,y", "", "\u00e9", "--", "--agent-mode", "x y"]
a = seen([rec], tricky, {}); b = seen([RC], tricky, {"RUSTY_CARGO_REAL": rec})
check("arguments and environment seen by cargo (exit 7 too)", (a[0], [l for l in a[1] if not l.startswith(b"argv0=")]), (b[0], [l for l in b[1] if not l.startswith(b"argv0=")]))
check("argv0 is cargo (so rustup picks the toolchain)", True, b"argv0=cargo" in b[1])
# RUSTY_AGENT with another value changes nothing
for v in ["0", "yes", "true", ""]:
    cwd = fx["errors3"]
    r1 = run(CARGO, ["build"] + J, cwd, target=tempfile.mkdtemp(dir=tmp)); r2 = run([RC, TOOLCHAIN], ["build"] + J, cwd, env=clean_env(RUSTY_AGENT=v), target=tempfile.mkdtemp(dir=tmp))
    check(f"RUSTY_AGENT={v!r} changes nothing", (r1[0], r1[2].replace(b"\n", b"")[:50]), (r2[0], r2[2].replace(b"\n", b"")[:50]))
# drop-in: a symlink named cargo first on PATH, used by another tool (make-like shell script and python)
env = clean_env(PATH=links + ":" + os.environ["PATH"])
script = os.path.join(tmp, "caller.sh"); open(script, "w").write("#!/bin/sh\ncargo +1.98.1 build -j4 --offline 2>&1; echo \"exit=$?\"\n"); os.chmod(script, 0o755)
t1 = tempfile.mkdtemp(dir=tmp); t2 = tempfile.mkdtemp(dir=tmp)
p1 = subprocess.run([script], cwd=fx["errors3"], capture_output=True, env=dict(clean_env(), CARGO_TARGET_DIR=t1))
p2 = subprocess.run([script], cwd=fx["errors3"], capture_output=True, env=dict(env, CARGO_TARGET_DIR=t2))
check("drop-in as cargo on PATH, called by a shell script", norm(p1.stdout, t1), norm(p2.stdout, t2))
w = subprocess.run(["sh", "-c", "command -v cargo"], env=env, capture_output=True, text=True).stdout.strip()
check("the symlink is what PATH finds first", links + "/cargo", w)
r = subprocess.run(["cargo", TOOLCHAIN, "--version"], env=env, capture_output=True)
check("symlinked cargo finds the real one, not itself (no loop)", subprocess.run(CARGO + ["--version"], capture_output=True).stdout, r.stdout)
for sub in (["clippy", "--version"], ["fmt", "--version"], ["metadata", "--help"]):
    a = subprocess.run(["cargo", TOOLCHAIN] + sub, env=env, capture_output=True); b = subprocess.run(CARGO + sub, env=clean_env(), capture_output=True)
    check("drop-in subcommand " + sub[0], (a.returncode, a.stdout, a.stderr), (b.returncode, b.stdout, b.stderr))
# artifacts byte-identical
t1 = tempfile.mkdtemp(dir=tmp); t2 = tempfile.mkdtemp(dir=tmp)
for exe, t in ((CARGO, t1), (RC, t2)):
    run(exe if exe is CARGO else [exe, TOOLCHAIN], ["build", "--release", "-j4", "--offline"], fx["clean"], target=t)
check("release binary bytes", sha(open(t1 + "/release/clean", "rb").read()), sha(open(t2 + "/release/clean", "rb").read()))
# overhead: --version through the rustup proxy, median of 30, alternating
def t(exe):
    s = time.perf_counter(); subprocess.run(exe + ["--version"], env=clean_env(), stdout=subprocess.DEVNULL); return (time.perf_counter() - s) * 1000
tc, tr = [], []
for _ in range(30): tc.append(t(CARGO)); tr.append(t([RC, TOOLCHAIN]))
over = statistics.median(tr) - statistics.median(tc)
print(f"overhead --version: cargo {statistics.median(tc):.2f} ms, rusty-cargo {statistics.median(tr):.2f} ms, diff {over:.2f} ms")
def t2(exe):
    s = time.perf_counter(); subprocess.run(exe + ["check", "-j4", "--offline"], cwd=fx["clean"], env=clean_env(), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL); return (time.perf_counter() - s) * 1000
tc, tr = [], []
for _ in range(30): tc.append(t2(CARGO)); tr.append(t2([RC, TOOLCHAIN]))
over2 = statistics.median(tr) - statistics.median(tc)
print(f"overhead up-to-date check: cargo {statistics.median(tc):.2f} ms, rusty-cargo {statistics.median(tr):.2f} ms, diff {over2:.2f} ms")
if max(over, over2) >= 2: print("FAIL overhead >= 2 ms"); fails += 1
print("FAILED" if fails else "ALL PASS"); sys.exit(1 if fails else 0)
