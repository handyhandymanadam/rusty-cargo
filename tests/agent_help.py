#!/usr/bin/env python3
"""Feature 14 and checklist item 3: self-description, version line, help section; every option opt-in; no option name collides
with cargo's or rustc's own option names."""
import json, os, re, subprocess, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
fails = 0
def check(name, cond, detail=""):
    global fails
    if not cond: fails += 1
    print(("ok   " if cond else "FAIL ") + name + ("" if cond else "  " + str(detail)[:600]))
tmp, fx = private_fixtures("clean", "errors3")
cwd = fx["clean"]
c = ["cargo", TOOLCHAIN]
r = subprocess.run([RC, TOOLCHAIN, "--agent-help"], capture_output=True, text=True, env=clean_env())
h = json.loads(r.stdout)
check("--agent-help works without agent mode: one JSON object, exit 0", r.returncode == 0 and h["schema"] == 1 and h["tool"] == "rusty-cargo")
check("options, records, codes and rules are all there", all(h[k] for k in ("options", "records", "codes", "rules")))
check("--agent-help documents RUSTY_AGENT_OUT and the dropped closing lines", any(e["name"].startswith("RUSTY_AGENT_OUT") for e in h["environment"]) and "could not compile" in h["dropped_lines"])
names = [o["name"].split("=")[0] for o in h["options"]]
# collisions with cargo and rustc option names
text = ""
for sub in ["", "build", "check", "test", "run", "clippy", "doc", "bench", "rustc", "rustdoc", "fix", "tree", "metadata", "fmt", "install", "add", "update", "clean", "new", "init", "publish", "package", "fetch", "vendor", "search", "login", "owner", "yank", "locate-project", "pkgid", "verify-project", "generate-lockfile", "remove", "report", "help"]:
    text += subprocess.run(c + ([sub] if sub else []) + ["--help"], capture_output=True, text=True, env=clean_env(), cwd=cwd).stdout
for flag in (["--help", "-v"], ["-Zhelp"], ["-Chelp"], ["-Whelp"]):
    p = subprocess.run(["rustc", TOOLCHAIN] + flag, capture_output=True, text=True, env=clean_env(), cwd=cwd); text += p.stdout + p.stderr
text += subprocess.run(["rustc", TOOLCHAIN, "-Zunstable-options", "--help", "-v"], capture_output=True, text=True, env=dict(clean_env(), RUSTC_BOOTSTRAP="1")).stdout
text += subprocess.run(["rustc", TOOLCHAIN, "-Zhelp"], capture_output=True, text=True, env=dict(clean_env(), RUSTC_BOOTSTRAP="1")).stdout
text += subprocess.run(["cargo", "+nightly", "-Zhelp"], capture_output=True, text=True, env=clean_env()).stdout
theirs = set(re.findall(r"--?[A-Za-z][A-Za-z0-9-]*", text))
check("collected a real set of cargo and rustc option names", len(theirs) > 400, len(theirs))
clash = [n for n in names if n in theirs]
check("no option name of ours is an option of cargo, its subcommands, rustc, -Z, -C or -W help", clash == [], clash)
# prefixes cargo might also accept as an abbreviation: none of its options starts with ours
check("no cargo/rustc option is a prefix-extension of ours either", [t for t in theirs if any(t.startswith(n) and t != n for n in names)] == [])
# --version
rc_ver = subprocess.run([RC, TOOLCHAIN, "--version"], capture_output=True, text=True, env=clean_env()).stdout
cargo_ver = subprocess.run(c + ["--version"], capture_output=True, text=True, env=clean_env()).stdout
check("--version without agent mode: cargo's bytes", rc_ver == cargo_ver)
av = subprocess.run([RC, TOOLCHAIN, "--version", "--agent-mode"], capture_output=True, text=True, env=clean_env()).stdout
check("--version in agent mode: cargo's text then the agent line", av == cargo_ver + "agent mode: yes, schema 1\n", av)
av = subprocess.run([RC, TOOLCHAIN, "-V"], capture_output=True, text=True, env=clean_env(RUSTY_AGENT="1")).stdout
check("-V with RUSTY_AGENT=1 too", av == cargo_ver + "agent mode: yes, schema 1\n")
av = subprocess.run([RC, TOOLCHAIN, "--version", "--verbose", "--agent-mode"], capture_output=True, text=True, env=clean_env()).stdout
check("--version --verbose keeps cargo's lines and adds ours last", av.startswith("cargo 1.98.1") and av.endswith("agent mode: yes, schema 1\n") and "release: 1.98.1" in av, av)
# --help
base = subprocess.run(c + ["build", "--help"], capture_output=True, text=True, env=clean_env()).stdout
plain = subprocess.run([RC, TOOLCHAIN, "build", "--help"], capture_output=True, text=True, env=clean_env()).stdout
agent = subprocess.run([RC, TOOLCHAIN, "build", "--help", "--agent-mode"], capture_output=True, text=True, env=clean_env()).stdout
check("--help without agent mode: cargo's bytes", plain == base)
check("--help in agent mode: cargo's text first, then the agent section naming every option", agent.startswith(base) and all(n in agent for n in names), agent[len(base):][:300])
# every option is opt-in: off without the flag/env; each option alone is stripped (cargo never errors on it)
code, out, err = run([RC, TOOLCHAIN], ["check", "--offline"], fx["errors3"], target=tempfile.mkdtemp(dir=tmp))
check("no flags: no records", records(err)[0] == [])
for o in h["options"]:
    n = o["name"]
    if n in ("--agent-help", "--no-agent-mode"): continue
    arg = n.replace("NAME", "serde_json")
    code, out, err = run([RC, TOOLCHAIN], ["check", arg, "--offline"], fx["errors3"], target=tempfile.mkdtemp(dir=tmp))
    check(f"{n}: not passed to cargo (cargo did not say 'unexpected argument')", b"unexpected argument" not in err and b"unrecognized" not in err, err[:200])
# only the flag-in-umbrella ones are on in --agent-mode: types seen
code, out, err = run([RC, TOOLCHAIN], ["test", "--agent-mode", "--offline", "--no-run"], fx["errors3"], target=tempfile.mkdtemp(dir=tmp))
types = {r["type"] for r in records(err)[0]}
check("--agent-mode alone gives diagnostic and summary only (no fix-preview, missing-crate, why-*, panic-report)", types <= {"diagnostic", "summary"} and "diagnostic" in types, types)
print("FAILED" if fails else "ALL PASS"); sys.exit(1 if fails else 0)
