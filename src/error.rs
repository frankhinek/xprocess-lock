use std::io;
use std::path::PathBuf;

use snafu::Snafu;

/// Crate-wide Result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Public error type for xprocess-lock.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum Error {
    /// Failed to acquire the OS file lock (exclusive/shared).
    #[snafu(display("Acquiring {mode} lock on {path:?} failed: {source}"))]
    AcquireLock { path: PathBuf, mode: &'static str, source: io::Error },

    /// Failed to create the parent directory for the lock file.
    #[snafu(display("Creating lock directory {path:?} failed: {source}"))]
    CreateDir { path: PathBuf, source: io::Error },

    /// The provided lock name was empty (after user input, before sanitization).
    #[snafu(display("Lock name cannot be empty"))]
    EmptyName,

    /// The background blocking task failed to run/return (Tokio only).
    #[cfg(feature = "async")]
    #[snafu(display("spawn_blocking join failed: {source}"))]
    JoinBlocking { source: tokio::task::JoinError },

    /// Failed to open/create the lock file.
    #[snafu(display("Opening lock file {path:?} failed: {source}"))]
    OpenLockFile { path: PathBuf, source: io::Error },
}
