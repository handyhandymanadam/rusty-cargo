//! Finds the real cargo: RUSTY_CARGO_REAL, else the first `cargo` on PATH that is not this program
//! (compared by resolved path and by device+inode, so a symlink or hard link named cargo is skipped).
use std::ffi::OsString;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;

pub fn find() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("RUSTY_CARGO_REAL").filter(|v| !v.is_empty()) { return Some(v.into()); }
    let me = std::env::current_exe().ok();
    let me_canon = me.as_ref().and_then(|p| p.canonicalize().ok());
    let me_id = me.as_ref().and_then(|p| std::fs::metadata(p).ok()).map(|m| (m.dev(), m.ino()));
    let path: OsString = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = if dir.as_os_str().is_empty() { PathBuf::from("cargo") } else { dir.join("cargo") };
        let Ok(md) = std::fs::metadata(&cand) else { continue };
        if !md.is_file() || md.permissions().mode() & 0o111 == 0 { continue; }
        if Some((md.dev(), md.ino())) == me_id { continue; }
        if let (Some(a), Ok(b)) = (&me_canon, cand.canonicalize()) { if *a == b { continue; } }
        return Some(cand);
    }
    None
}
