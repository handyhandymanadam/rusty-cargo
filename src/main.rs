//! rusty-cargo: thin driver in front of cargo. Without agent mode it is a pure exec.
mod codes;
mod config;
mod diag;
mod driver;
mod explain;
mod features;
mod fixpreview;
mod help;
mod ownership;
mod panicreport;
mod paths;
mod real;
mod rootcause;
mod shape;
mod testreport;
mod whyfeature;
mod whyrebuilt;

use std::env;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::process::Command;

fn main() {
    let mut args = env::args_os();
    let _invoked = args.next();
    let args: Vec<OsString> = args.collect();
    let parsed = config::parse(args);
    let Some(real) = real::find() else {
        eprintln!("rusty-cargo: cannot find the real cargo (set RUSTY_CARGO_REAL)");
        std::process::exit(127);
    };
    if parsed.cfg.agent_help { println!("{}", help::agent_help()); std::process::exit(0); }
    if parsed.cfg.needs_driver() {
        std::process::exit(driver::run(&parsed.cfg, &real, parsed.args));
    }
    // Default path: nothing else is looked at. exec keeps stdio, environment, signals, exit code.
    // argv[0] is "cargo" so that the rustup proxy picks the toolchain the way it does for plain cargo.
    let err = Command::new(&real).arg0("cargo").args(&parsed.args).exec();
    eprintln!("rusty-cargo: cannot run {}: {err}", real.display());
    std::process::exit(127);
}
