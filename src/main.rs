//! manzil — minimalist file linker.
//!
//! Reads a new manifest (and optionally an old one), then reconciles symlinks
//! in the user's home directory. Atomic per-file, mutually-exclusive per-user.
//!
//! Usage: manzil <new-manifest.json> [<old-manifest.json>]

use std::collections::HashSet;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind};
use std::os::unix::fs::symlink;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{self, ExitCode};

use serde::Deserialize;

#[derive(Deserialize)]
struct Manifest {
    #[serde(default)]
    files: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    target: PathBuf,
    source: PathBuf,
    #[serde(default)]
    clobber: bool,
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let (new, old) = match args.as_slice() {
        [n]    => (Path::new(n), None),
        [n, o] => (Path::new(n), Some(Path::new(o))),
        _ => {
            eprintln!("usage: manzil <new-manifest.json> [<old-manifest.json>]");
            return ExitCode::from(2);
        }
    };

    match run(new, old) {
        Ok(0)        => ExitCode::SUCCESS,
        Ok(failures) => {
            eprintln!("manzil: {failures} entry(s) failed");
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("manzil: fatal: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(new_path: &Path, old_path: Option<&Path>) -> io::Result<u32> {
    let new = read_manifest(new_path)?;
    let old = match old_path {
        Some(p) if p.exists() => Some(read_manifest(p)?),
        _ => None,
    };

    // Hold an exclusive lock for the duration of this run. Released on drop.
    let _lock = acquire_lock()?;

    let mut failures = 0u32;
    let mut report = |target: &Path, res: io::Result<()>| match res {
        Ok(()) => {}
        Err(e) => {
            eprintln!("manzil: {}: {e}", target.display());
            failures += 1;
        }
    };

    // Prune: targets in old but not in new.
    if let Some(om) = old {
        let keep: HashSet<&Path> = new.files.iter().map(|e| e.target.as_path()).collect();
        for entry in &om.files {
            if !keep.contains(entry.target.as_path()) {
                let r = prune(&entry.target);
                report(&entry.target, r);
            }
        }
    }

    // Activate: every entry in new.
    for entry in &new.files {
        let r = activate(entry);
        report(&entry.target, r);
    }

    Ok(failures)
}

fn read_manifest(path: &Path) -> io::Result<Manifest> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(ErrorKind::InvalidData, e))
}

/// Remove `target` iff it is still a symlink we own. Regular files left alone.
fn prune(target: &Path) -> io::Result<()> {
    match fs::symlink_metadata(target) {
        Ok(m) if m.file_type().is_symlink() => {
            fs::remove_file(target)?;
            eprintln!("manzil: removed {}", target.display());
            Ok(())
        }
        Ok(_) => {
            eprintln!(
                "manzil: warning: stale entry {} is not a symlink, leaving alone",
                target.display()
            );
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Ensure `target` is a symlink pointing at `entry.source`.
fn activate(entry: &Entry) -> io::Result<()> {
    if let Some(parent) = entry.target.parent() {
        fs::create_dir_all(parent)?;
    }

    match fs::symlink_metadata(&entry.target) {
        // Already a symlink → idempotent update.
        Ok(m) if m.file_type().is_symlink() => {
            if fs::read_link(&entry.target).ok().as_deref() == Some(entry.source.as_path()) {
                return Ok(()); // already correct, silent no-op
            }
            atomic_symlink(&entry.source, &entry.target)
        }
        // Real file/dir present, clobber requested → replace.
        Ok(m) if entry.clobber => {
            if m.is_dir() {
                fs::remove_dir_all(&entry.target)?;
            } else {
                fs::remove_file(&entry.target)?;
            }
            atomic_symlink(&entry.source, &entry.target)?;
            eprintln!("manzil: clobbered {}", entry.target.display());
            Ok(())
        }
        // Real file/dir present, no clobber → preserve.
        Ok(_) => {
            eprintln!(
                "manzil: warning: {} exists and is not a symlink (set clobber = true to override)",
                entry.target.display()
            );
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::NotFound => atomic_symlink(&entry.source, &entry.target),
        Err(e) => Err(e),
    }
}

/// Atomically place a symlink at `target` via `symlinkat` to a sibling tmp path
/// followed by `rename(2)` — atomic for any file/symlink on the same filesystem.
fn atomic_symlink(source: &Path, target: &Path) -> io::Result<()> {
    let tmp = sibling_tmp(target);
    // Best-effort cleanup of any leftover tmp from a prior crashed run.
    let _ = fs::remove_file(&tmp);

    symlink(source, &tmp)?;
    if let Err(e) = fs::rename(&tmp, target) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

fn sibling_tmp(target: &Path) -> PathBuf {
    let mut name = target
        .file_name()
        .map(|n| n.to_owned())
        .unwrap_or_default();
    name.push(format!(".manzil-tmp.{}", process::id()));
    target.with_file_name(name)
}

/// Acquire an exclusive flock at `$HOME/.local/state/manzil/lock`, blocking
/// until available. Released when the returned `File` is dropped.
fn acquire_lock() -> io::Result<File> {
    let home = env::var_os("HOME")
        .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "HOME not set"))?;
    let dir = PathBuf::from(home).join(".local/state/manzil");
    fs::create_dir_all(&dir)?;
    let path = dir.join("lock");
    let f = OpenOptions::new().create(true).write(true).truncate(false).open(&path)?;
    // SAFETY: fd is valid for the duration of the call.
    let r = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) };
    if r != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(f)
}
