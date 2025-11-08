//! Cross-platform cross-process file locking using only the Rust standard library.
//!
//! Typical pattern:
//! - Each test: `lock_shared()` and hold the guard for the test duration.
//! - One finalizer: take `lock_exclusive()` *after* readers drop and perform teardown.

#![forbid(unsafe_code)]

#[cfg(all(feature = "async", feature = "blocking"))]
compile_error!("\"async\" and \"blocking\" features cannot be enabled at the same time.");
#[cfg(not(any(feature = "async", feature = "blocking")))]
compile_error!("Enable exactly one of: feature \"async\" or feature \"blocking\".");

mod error;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::{env, io};

use snafu::ResultExt; // for .context(...)

pub use crate::error::{Error, Result};

// ============================ Public API types ============================

/// Guard holding a shared/exclusive OS file lock (drops = releases the lock).
#[derive(Debug)]
pub struct LockGuard(#[allow(dead_code)] File);

impl LockGuard {
    /// Convenience: explicitly release the lock.
    pub fn unlock(self) {
        drop(self);
    }
}

/// Named, cross‑process lock. The `name` becomes `<base>/<sanitized>.lock`.
#[derive(Debug)]
pub struct XProcessLock {
    lock_file: PathBuf,
}

impl XProcessLock {
    /// Create a lock scope identified by `name`.
    pub fn create(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        if name.trim().is_empty() {
            return error::EmptyNameSnafu.fail();
        }
        let name = format!("{}.lock", sanitize(&name));
        let lock_file = default_base_dir().join(name);
        Ok(Self { lock_file })
    }
}

// ============================ Async API ============================

#[cfg(feature = "async")]
impl XProcessLock {
    /// Take an **exclusive** lock (blocks until all shared holders release).
    pub async fn lock_exclusive(&self) -> Result<LockGuard> {
        let guard = open_locked_async(self.lock_file.clone(), LockMode::Exclusive).await?;
        Ok(LockGuard(guard))
    }

    /// Take a **shared** (read) lock for the duration of your test/work.
    pub async fn lock_shared(&self) -> Result<LockGuard> {
        let guard = open_locked_async(self.lock_file.clone(), LockMode::Shared).await?;
        Ok(LockGuard(guard))
    }
}

// ============================ Sync API ============================

#[cfg(feature = "blocking")]
impl XProcessLock {
    /// Take an **exclusive** lock (blocks until all shared holders release).
    pub fn lock_exclusive(&self) -> Result<LockGuard> {
        let guard = open_locked(&self.lock_file, LockMode::Exclusive)?;
        Ok(LockGuard(guard))
    }

    /// Take a **shared** (read) lock for the duration of your test/work.
    pub fn lock_shared(&self) -> Result<LockGuard> {
        let guard = open_locked(&self.lock_file, LockMode::Shared)?;
        Ok(LockGuard(guard))
    }
}

// ============================ Private helpers ============================

#[derive(Copy, Clone)]
enum LockMode {
    Exclusive,
    Shared,
}

fn default_base_dir() -> PathBuf {
    env::var_os("XPROCESS_LOCK_DIR").map(PathBuf::from).unwrap_or_else(|| env::temp_dir().join("xprocess-lock"))
}

/// Open (create if needed) and lock the file (blocking).
fn open_locked(path: &Path, mode: LockMode) -> Result<File> {
    // Ensure directory exists.
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).context(error::CreateDirSnafu { path: dir.to_path_buf() })?;
    }

    let f = open_lock_file(path).context(error::OpenLockFileSnafu { path: path.to_path_buf() })?;

    match mode {
        LockMode::Exclusive => {
            f.lock().context(error::AcquireLockSnafu { path: path.to_path_buf(), mode: "exclusive" })?
        }
        LockMode::Shared => {
            f.lock_shared().context(error::AcquireLockSnafu { path: path.to_path_buf(), mode: "shared" })?
        }
    }
    Ok(f)
}

#[cfg(feature = "async")]
async fn open_locked_async(path: PathBuf, mode: LockMode) -> Result<File> {
    use error::JoinBlockingSnafu;

    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await.context(error::CreateDirSnafu { path: dir.to_path_buf() })?;
    }
    // Run the blocking open+lock sequence off the runtime thread.
    tokio::task::spawn_blocking(move || open_locked(&path, mode)).await.context(JoinBlockingSnafu)?
}

fn open_lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true) // needed on some platforms for locking semantics
        .write(true) // Windows file locking requires write access
        .create(true) // create the lock file if it doesn't exist
        .truncate(false) // do NOT clobber an existing file
        .open(path)
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

// ============================ Tests ============================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_dir_respects_env_override() {
        temp_env::with_var("XPROCESS_LOCK_DIR", Some("/tmp/custom"), || {
            assert_eq!(default_base_dir(), PathBuf::from("/tmp/custom"));
        });
    }

    #[test]
    fn default_dir_uses_temp_when_unset() {
        temp_env::with_var_unset("XPROCESS_LOCK_DIR", || {
            assert_eq!(default_base_dir(), std::env::temp_dir().join("xprocess-lock"));
        });
    }

    #[test]
    fn sanitize_preserves_allowed_chars() {
        let input = "Safe-Name_123";
        assert_eq!(sanitize(input), input);
    }

    #[test]
    fn sanitize_replaces_disallowed_ascii() {
        assert_eq!(sanitize("bad name!"), "bad_name_");
        assert_eq!(sanitize("foo.bar"), "foo_bar");
    }

    #[test]
    fn sanitize_replaces_non_ascii() {
        assert_eq!(sanitize("über"), "_ber");
        assert_eq!(sanitize("你好"), "__");
    }

    #[test]
    fn create_rejects_empty_or_whitespace_names() {
        assert!(matches!(XProcessLock::create(""), Err(Error::EmptyName)));
        assert!(matches!(XProcessLock::create("   "), Err(Error::EmptyName)));
    }
}
