//! Safe wrappers for platform-specific unsafe syscall operations.
//!
//! All `libc` syscall wrappers and `std::env` unsafe calls live here.
//! The only other `unsafe` in the codebase is `unsafe impl Send` for
//! `SendableStream` in `audio::capture` (required by the CPAL stream API).

use std::ffi::CStr;
use std::sync::Mutex;

/// Serializes all calls to [`set_env`] and [`remove_env`].
///
/// `std::env::set_var` / `std::env::remove_var` are globally unsound when any
/// other thread concurrently reads environment variables (e.g. via `getenv`).
/// Rust stabilized the deprecation warning in 1.81.  While we cannot prevent
/// third-party C libraries (ashpd/zbus) from reading the environment at any
/// time, we can at least guarantee that **our own writes are never concurrent
/// with each other**, eliminating the writer–writer race.
static ENV_WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Return the effective user ID of the calling process.
///
/// # Safety
/// `getuid` is a read-only POSIX syscall with no preconditions.
pub fn current_uid() -> u32 {
    // SAFETY: getuid is a read-only POSIX syscall with no preconditions.
    unsafe { libc::getuid() }
}

/// Return available disk space in megabytes for the filesystem containing `path`.
///
/// Returns `None` if the `statvfs` call fails (e.g. path does not exist).
///
/// # Safety
/// `statvfs` is a standard POSIX call; we pass a valid `CStr` and a zeroed
/// struct, then check the return value before reading fields.
pub fn available_disk_mb(path: &CStr) -> Option<u64> {
    // SAFETY: statvfs is a standard POSIX call; we pass a valid CStr and a
    // zeroed struct, then check the return value before reading fields.
    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(path.as_ptr(), &mut stat) != 0 {
            return None;
        }
        Some(stat.f_bavail.saturating_mul(stat.f_frsize) / (1024 * 1024))
    }
}

/// Set an environment variable.
///
/// All writes are serialized through [`ENV_WRITE_LOCK`] to eliminate
/// concurrent-writer races.  A residual reader–writer race remains: C libraries
/// such as `zbus`/`ashpd` may call `getenv("DBUS_SESSION_BUS_ADDRESS")` at any
/// time.  This is unavoidable because those libraries offer no API to pass the
/// address explicitly.  In practice the race window is short (a single pointer
/// swap in glibc's `setenv`), and the worst outcome is that the library picks up
/// a stale address and returns a connection error — which the portal reconnect
/// path already handles.
///
/// Prefer calling this before spawning threads whenever possible.
pub fn set_env(key: &str, value: &str) {
    let _guard = ENV_WRITE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: Writer–writer races are eliminated by ENV_WRITE_LOCK.
    // The remaining reader–writer race with C library getenv() is unavoidable
    // without API support from the library; see the doc comment above.
    unsafe {
        std::env::set_var(key, value);
    }
}

/// Remove an environment variable.
///
/// Same serialization guarantee as [`set_env`]; see that function's doc comment
/// for a discussion of the residual reader–writer risk.
pub fn remove_env(key: &str) {
    let _guard = ENV_WRITE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: Writer–writer races are eliminated by ENV_WRITE_LOCK.
    // See set_env for the full safety discussion.
    unsafe {
        std::env::remove_var(key);
    }
}

/// Suppress noisy JACK/ALSA/PipeWire messages during audio backend probing.
///
/// Must be called before spawning threads.
pub fn suppress_audio_warnings() {
    // SAFETY: Called at startup before any threads are spawned.
    set_env("JACK_NO_START_SERVER", "1");
    set_env("JACK_NO_AUDIO_RESERVATION", "1");
    set_env("PIPEWIRE_DEBUG", "0");
    set_env("ALSA_DEBUG", "0");
    set_env("PW_LOG", "0");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn current_uid_is_valid_posix_uid() {
        // getuid() has no failure mode on a POSIX system; it always returns the
        // caller's effective UID.  On Linux, UIDs are 32-bit values (0 = root,
        // 65534 = nobody), so we assert the returned value is in the u32 range —
        // which is already guaranteed by the type — and that a second call is
        // stable (same process, same UID).
        let uid = current_uid();
        assert_eq!(
            uid,
            current_uid(),
            "getuid() must be stable within a process"
        );
    }

    #[test]
    fn available_disk_mb_root_returns_nonzero() {
        let path = CStr::from_bytes_with_nul(b"/\0").expect("valid CStr");
        let mb = available_disk_mb(path).expect("expected Some for root filesystem");
        // The root filesystem must have at least 1 MB free on any CI or dev machine.
        assert!(mb > 0, "expected > 0 MB free on root filesystem, got {mb}");
    }

    #[test]
    fn available_disk_mb_invalid_returns_none() {
        let path = CStr::from_bytes_with_nul(b"/nonexistent_path_that_does_not_exist_xyz\0")
            .expect("valid CStr");
        let result = available_disk_mb(path);
        assert_eq!(result, None, "expected None for nonexistent path");
    }

    #[test]
    fn set_env_and_read_back() {
        let _guard = ENV_LOCK.lock().expect("ENV_LOCK poisoned");
        const KEY: &str = "VOICSH_SYS_TEST_VAR";
        set_env(KEY, "hello");
        let value = std::env::var(KEY).expect("var should be set");
        assert_eq!(value, "hello");
        remove_env(KEY);
        assert!(
            std::env::var(KEY).is_err(),
            "var should be removed after remove_env"
        );
    }
}
