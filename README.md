# xprocess-lock

**Cross‑process file locks** for test orchestration, built on the Rust standard library’s file locks
(Rust 1.89+). Use it to:

- Gate **global setup** (container start, DB seed) with an **exclusive** lock
- Let each test hold a **shared** lock while it runs
- Elect **one finalizer** to take an **exclusive** lock after all tests finish and tear down

Typical use case: start a Docker container **exactly once** for an entire test run, even when tests
run in parallel processes, and perform **teardown** only after all tests finish.

<!--[![Crates.io](https://img.shields.io/crates/v/xprocess-lock.svg?style=flat&labelColor=532100&color=D95400&logo=Rust&logoColor=white)](https://crates.io/crates/xprocess-lock)-->
[![CI](https://img.shields.io/github/actions/workflow/status/frankhinek/xprocess-lock/ci.yml?style=flat&labelColor=532100&color=D95400&logo=GitHub%20Actions&logoColor=white)](https://github.com/frankhinek/xprocess-lock/actions/workflows/ci.yml)
<!--[![Documentation](https://docs.rs/xprocess-lock/badge.svg)](https://docs.rs/xprocess-lock)-->

---

## Features

- **Cross‑platform**: Linux, macOS, Windows
- **Standard library** locks `std::fs::File::{lock, lock_shared}` (no extra locking deps)
- **Shared / Exclusive** modes (reader/writer style)
- **Async** [`tokio`](https://docs.rs/tokio) *or* **blocking** APIs via feature flags (mutually exclusive)
- **Clear errors** with [`snafu`](https://docs.rs/snafu)
- **Zero unsafe**

**MSRV:** Rust 1.89+ (for `File::lock` / `lock_shared`).
**Default feature:** `async`.

---

## Install

```toml
[dependencies]
xprocess-lock = { version = "0.1" } # async by default

# Or, for blocking (no tokio):
# xprocess-lock = { version = "0.1", default-features = false, features = ["blocking"] }
````

Feature flags:

* `async` (default): enables `tokio` and `async fn` APIs
* `blocking`: enables synchronous APIs
* `async` and `blocking` are **mutually exclusive**

---

## Quick start

### Asynchronous (Tokio)

```rust
use xprocess_lock::{XProcessLock, Result};

#[tokio::main]
async fn main() -> Result<()> {
    // The lock filename is "<base>/<sanitized-name>.lock"
    // Base dir: $XPROCESS_LOCK_DIR or <tmp>/xprocess-lock
    let lock = XProcessLock::create("redis")?;

    // Hold a shared lock for the duration of the work
    let _guard = lock.lock_shared().await?;

    // ... do test/work here while holding the lock ...

    // Lock released when `_guard` is dropped (RAII)
    Ok(())
}
```

### Blocking (sync)

```rust
use xprocess_lock::{XProcessLock, Result};

fn main() -> Result<()> {
    let lock = XProcessLock::create("redis")?;
    let _guard = lock.lock_shared()?;
    // ... work under the shared lock ...
    Ok(())
}
```

---

## Orchestrating tests

This crate intentionally keeps the API small: **no closures** inside the lock. You compose the
pattern yourself. What follows are examples of how to use the crate to orchestrate tests.

### Gated setup (async)

If you need to prevent teardown from sneaking in *between* “start container” and “join shared,”
use a second lock name as a **setup gate**:

```rust
use xprocess_lock::{XProcessLock, Result};

#[tokio::test]
async fn test_whatever() -> Result<()> {
    let setup = XProcessLock::create("setup-db")?;   // a separate filename
    let setup_guard = setup.lock_exclusive().await?; // serialize creation across processes

    // First process that acquires the setup lock, starts the container
    let container = start_redis().await?

    let in_use = XProcessLock::create("use-db")?;
    let _shared = in_use.lock_shared().await?;       // join as a reader while setup gate is held
    drop(setup_guard);                               // now allow others to proceed

    // ... test logic ...

    // Drop shared lock at test end
    drop(_shared);

    // Wait for ALL tests to finish (no readers) then tear down
    let _exclusive = in_use.lock_exclusive().await?;

    // stop/remove the container
    container.stop().await?;
    container.rm().await?;
}
```

This avoids a timing window where teardown could take exclusive before you’ve joined the shared set.

### Finalizer election (async)

Use a small file sentinel to elect exactly one finalizer, then wait for exclusive:

```rust
use std::{io, path::PathBuf};
use tokio::fs::OpenOptions;
use xprocess_lock::{XProcessLock, Result};

async fn try_become_finalizer(marker: &PathBuf) -> Result<bool> {
    match OpenOptions::new().write(true).create_new(true).open(marker).await {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e.into()),
    }
}

#[tokio::test]
async fn suite_piece() -> Result<()> {
    let in_use = XProcessLock::create("redis")?;
    let _guard = in_use.lock_shared().await?;

    // ... test logic ...

    // Drop shared lock at test end
    drop(_guard);

    // Elect a single finalizer (first to create wins)
    let marker = std::env::temp_dir().join("xprocess-lock").join("redis.teardown.once");
    if try_become_finalizer(&marker).await? {
        // Wait for ALL tests to finish (no readers) then tear down
        let _ex = in_use.lock_exclusive().await?;
        // stop/remove the container, drop temp data, etc.
    }
    Ok(())
}
```

---

## Behavior & caveats

* **Advisory locks**: All cooperating processes must use the same file to coordinate. On Linux/macOS
  [`flock(2)`](https://linux.die.net/man/2/flock) is used and on Windows
  [`LockFileEx`](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex).
* **Shared + Exclusive**: Exclusive blocks until **all** shared holders drop; shared blocks while an
  exclusive holder exists.
* **Network filesystems**: File locking semantics can vary on NFS/SMB. Prefer local disks in CI.
* **MSRV**: Rust **1.89+** (stabilized `File::lock`/`lock_shared`).

---

## Configuration

* **Name**: `XProcessLock::create("name")`
  All cooperating processes must use the same name.
* **Base directory**: by default, `<tmp>/xprocess-lock`.
  Override with `XPROCESS_LOCK_DIR=/custom/path`.
* **Lock filename**: `<base>/<sanitized-name>.lock`.

---

## Frequently Asked Questions

### Why not just `OnceCell`?

`OnceCell`/`OnceLock` are **in-process** only. Test runners like nextest run tests in
**multiple processes**, so you need an **OS-visible** lock to prevent duplicate work.
`xprocess-lock` uses file locks for true cross-process coordination.

### Why not use [`named-lock`](https://docs.rs/named-lock/0.4.1/named_lock/index.html)?

`named-lock` is a good crate if what you want is a **single named, exclusive lock** across
processes. This crate is different and intentionally more minimal/opinionated:

* **Standard library locks**: `xprocess-lock` uses `std::fs::File::{lock, lock_shared}` (stable in
  Rust 1.89), no OS‑specific FFI, no extra backends or transitive dependencies.
* **Shared *and* exclusive**: this crate exposes explicit reader/writer semantics (shared vs
  exclusive) on a single file. That pattern maps cleanly to “tests hold shared; finalizer takes
  exclusive.”
* **Filesystem‑scoped**: you control where lock files live via `XPROCESS_LOCK_DIR`. That’s handy in
  CI (e.g., per workspace/job isolation) and easy to inspect (`ls` the directory).
* **Simplicity over reentrancy**: `named-lock` does additional handle bookkeeping to guard against
  re‑locking edge cases within a single process. `xprocess-lock` keeps the RAII contract simple:
  **one guard = one OS lock**; drop releases it.

Choose:
* **`named-lock`** if you want a general named mutex and don’t need shared locks.
* **`xprocess-lock`** if you want **reader/writer semantics** with the smallest possible API
  backed by the **standard library**—especially for test orchestration patterns.

---

## License

This project is licensed under the [MIT license](LICENSE).

---

## Contributing

Issues and PRs welcome!
