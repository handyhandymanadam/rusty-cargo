#!/usr/bin/env python3
"""Item A: RUSTY_AGENT_FD=N appends the same JSON-lines records to an already-open descriptor (like RUSTY_AGENT_OUT).
Runs the real rusty-cargo and real cargo on a fixture; every line is parsed as JSON."""
import json, os, subprocess, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
fails = 0
def check(name, cond, detail=""):
    global fails
    if not cond: fails += 1
    print(("ok   " if cond else "FAIL ") + name + ("" if cond else "  " + str(detail)[:700]))
tmp, fx = private_fixtures("errors3", "clean")
def go(fixture, args=("build",), flags=("--agent-mode",), env=None, pass_fds=(), **extra):
    e = dict(env or clean_env()); e.update(extra)
    e.setdefault("RUSTY_AGENT_DETAIL", "full")
    e["CARGO_TARGET_DIR"] = tempfile.mkdtemp(dir=tmp)
    p = subprocess.run([RC, TOOLCHAIN] + list(args) + list(flags) + ["-j4", "--offline"], cwd=fx[fixture], capture_output=True, env=e, pass_fds=pass_fds)
    return p.returncode, p.stdout, p.stderr
def fd_of(path, flags=os.O_WRONLY | os.O_CREAT | os.O_APPEND): return os.open(path, flags, 0o644)
def filerecs(path): return [json.loads(l) for l in open(path).read().splitlines()]

# 1. valid fd
f1 = os.path.join(tmp, "fd.jsonl"); fd = fd_of(f1)
code, out, err = go("errors3", RUSTY_AGENT_FD=str(fd), pass_fds=(fd,)); os.close(fd)
srec, other = records(err)
check("valid fd: file has every record, same objects in the same order as stderr", filerecs(f1) == srec and len(srec) == 4, (len(filerecs(f1)), len(srec)))
check("valid fd: exit code is cargo's (101), no warning record", code == 101 and not [r for r in srec if r.get("code") == "R0500"])
c0, o0, e0 = go("errors3")
check("valid fd: stderr records equal the run without the variable (same types and codes)", [(r["type"], r.get("code")) for r in srec] == [(r["type"], r.get("code")) for r in records(e0)[0]] and o0 == out)
r, w = os.pipe(); code, out, err = go("errors3", RUSTY_AGENT_FD=str(w), pass_fds=(w,)); os.close(w)
got = os.read(r, 1 << 22).decode().splitlines(); os.close(r)
check("valid fd: a pipe gets the records too", [json.loads(l) for l in got] == records(err)[0] and len(got) == 4, len(got))
n = len(filerecs(f1)); fd = fd_of(f1); code, out, err = go("clean", RUSTY_AGENT_FD=str(fd), pass_fds=(fd,)); os.close(fd)
check("valid fd: a second run appends", len(filerecs(f1)) == n + len(records(err)[0]) and code == 0)

# 2. bad values: one warning record, records still on stderr, exit code unchanged
def bad(name, value, why, pass_fds=()):
    code, out, err = go("errors3", RUSTY_AGENT_FD=value, pass_fds=pass_fds)
    rr, other = records(err); w = [x for x in rr if x.get("code") == "R0500"]
    check(f"bad fd ({name}): one R0500 warning record naming RUSTY_AGENT_FD, the 4 normal records follow, no plain-text noise, exit 101",
          len(w) == 1 and w[0]["severity"] == "warning" and "RUSTY_AGENT_FD" in w[0]["message"] and why in w[0]["message"] and len(rr) == 5 and code == 101 and not [l for l in other if "RUSTY_AGENT_FD" in l], (err[:500], code))
bad("not a number", "abc", "not a decimal number")
bad("empty", "", "not a decimal number")
bad("negative", "-1", "not a decimal number")
bad("too large", "99999999999999999999999", "too large")
bad("stderr", "2", "below 3")
bad("closed", "57", "not open")
rf = os.open(os.path.join(FX, "errors3/Cargo.toml"), os.O_RDONLY); bad("read-only", str(rf), "reading only", (rf,)); os.close(rf)
code, out, err = go("clean", RUSTY_AGENT_FD="57")
check("closed fd: a build that succeeds still exits 0", code == 0 and len([x for x in records(err)[0] if x.get("code") == "R0500"]) == 1, err[:300])

# 3. both OUT and FD
fo = os.path.join(tmp, "o.jsonl"); ff = os.path.join(tmp, "f.jsonl"); fd = fd_of(ff)
code, out, err = go("errors3", RUSTY_AGENT_OUT=fo, RUSTY_AGENT_FD=str(fd), pass_fds=(fd,)); os.close(fd)
srec = records(err)[0]
check("OUT and FD both set: both get every record, equal to stderr", filerecs(fo) == filerecs(ff) == srec and len(srec) == 4, (len(filerecs(fo)), len(filerecs(ff)), len(srec)))
fo2 = os.path.join(tmp, "o2.jsonl"); code, out, err = go("errors3", RUSTY_AGENT_OUT=fo2, RUSTY_AGENT_FD="57")
check("OUT good, FD closed: the file also gets the R0500 warning", [x.get("code") for x in filerecs(fo2)].count("R0500") == 1 and filerecs(fo2) == records(err)[0] and code == 101)
fd = fd_of(os.path.join(tmp, "g.jsonl")); code, out, err = go("errors3", RUSTY_AGENT_OUT="/nonexistent-dir/x", RUSTY_AGENT_FD=str(fd), pass_fds=(fd,)); os.close(fd)
check("OUT bad, FD good: OUT's note on stderr as before, the descriptor gets all records", b"cannot open RUSTY_AGENT_OUT" in err and filerecs(os.path.join(tmp, "g.jsonl")) == records(err)[0] and code == 101)

# 4. agent mode off
f3 = os.path.join(tmp, "off.jsonl"); fd = fd_of(f3)
plain = go("errors3", flags=(), env=clean_env())
off = go("errors3", flags=(), RUSTY_AGENT_FD=str(fd), pass_fds=(fd,)); os.close(fd)
check("agent off: output equals the run without the variable; nothing written to the descriptor", norm(off[2]) == norm(plain[2]) and off[1] == plain[1] and off[0] == plain[0] and os.path.getsize(f3) == 0)
bad_off = go("errors3", flags=(), RUSTY_AGENT_FD="57")
check("agent off: a closed fd gives no warning, output unchanged", b"R0500" not in bad_off[2] and norm(bad_off[2]) == norm(plain[2]))
fd = fd_of(f3); r2 = go("errors3", flags=("--no-agent-mode",), env=clean_env(RUSTY_AGENT="1"), RUSTY_AGENT_FD=str(fd), pass_fds=(fd,)); os.close(fd)
check("--no-agent-mode over RUSTY_AGENT=1: descriptor untouched, no records", os.path.getsize(f3) == 0 and not records(r2[2])[0])

# 5. children do not inherit RUSTY_AGENT_FD (a fake cargo records its environment)
fake = os.path.join(tmp, "fakecargo"); envdump = os.path.join(tmp, "childenv")
open(fake, "w").write(f"#!/bin/sh\nenv | grep '^RUSTY_AGENT' >> {envdump}\nexit 0\n"); os.chmod(fake, 0o755)
fd = fd_of(os.path.join(tmp, "c.jsonl"))
e = clean_env(); e.update(RUSTY_AGENT_FD=str(fd), RUSTY_AGENT_OUT=os.path.join(tmp, "c2.jsonl"), RUSTY_CARGO_REAL=fake)
for args in (["build"], ["--version"], ["build", "--explain-feature=x"]):
    p = subprocess.run([RC, "--agent-mode"] + args, cwd=fx["clean"], capture_output=True, env=e, pass_fds=(fd,))
ev = open(envdump).read()
check("cargo child (build, --version, feature query) gets RUSTY_AGENT=1 but not RUSTY_AGENT_FD", "RUSTY_AGENT=1" in ev and "RUSTY_AGENT_FD" not in ev, ev)
os.close(fd)

# 6. docs
h = json.loads(subprocess.run([RC, "--agent-help"], capture_output=True, env=clean_env()).stdout)
check("--agent-help documents RUSTY_AGENT_FD and R0500", any(x["name"].startswith("RUSTY_AGENT_FD") for x in h["environment"]) and any(c["code"] == "R0500" for c in h["codes"]))
doc = open(os.path.join(ROOT, "AGENT-MODE.md")).read()
check("AGENT-MODE.md documents RUSTY_AGENT_FD and R0500", "RUSTY_AGENT_FD" in doc and "R0500" in doc)
print("FAILED %d" % fails if fails else "ALL PASS"); sys.exit(1 if fails else 0)
