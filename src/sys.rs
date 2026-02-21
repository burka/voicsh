//! Safe wrappers for platform-specific unsafe operations.
//!
//! Every `unsafe` block in the codebase lives here. Call sites use the safe
//! public API and never touch `unsafe` directly.

use std::ffi::CStr;

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

/// Run a closure with stderr temporarily redirected to `/dev/null`.
///
/// This suppresses noisy ALSA/JACK/PipeWire messages that CPAL triggers
/// when probing audio backends. The messages are harmless but confusing to users.
///
/// # Safety
/// Uses `libc::dup`/`libc::dup2` to save and restore file descriptor 2 (stderr).
/// Safe as long as no other thread is concurrently manipulating fd 2.
pub fn with_suppressed_stderr<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    // SAFETY: Safe as long as no other thread is concurrently manipulating fd 2.
    unsafe {
        let saved_fd = libc::dup(2);
        let devnull = libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY);
        if saved_fd >= 0 && devnull >= 0 {
            libc::dup2(devnull, 2);
            libc::close(devnull);
        }

        let result = f();

        if saved_fd >= 0 {
            libc::dup2(saved_fd, 2);
            libc::close(saved_fd);
        }

        result
    }
}

/// Set an environment variable.
///
/// # Safety
/// Caller must ensure no other threads are reading environment variables concurrently.
pub fn set_env(key: &str, value: &str) {
    // SAFETY: Caller must ensure no other threads are reading environment
    // variables concurrently.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var(key, value);
    }
}

/// Remove an environment variable.
///
/// # Safety
/// Caller must ensure no other threads are reading environment variables concurrently.
pub fn remove_env(key: &str) {
    // SAFETY: Caller must ensure no other threads are reading environment
    // variables concurrently.
    #[allow(unsafe_code)]
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
    fn current_uid_does_not_panic() {
        // "Doesn't panic" is sufficient here: getuid() has no failure mode on
        // a standard POSIX system and returns whatever the OS reports.
        let uid = current_uid();
        // On any real system the result is a valid u32; just ensure it compiles
        // and runs without UB.
        let _ = uid;
    }

    #[test]
    fn available_disk_mb_root_returns_some() {
        let path = CStr::from_bytes_with_nul(b"/\0").expect("valid CStr");
        let result = available_disk_mb(path);
        assert!(
            result.is_some(),
            "expected Some for root filesystem, got None"
        );
    }

    #[test]
    fn available_disk_mb_invalid_returns_none() {
        let path = CStr::from_bytes_with_nul(b"/nonexistent_path_that_does_not_exist_xyz\0")
            .expect("valid CStr");
        let result = available_disk_mb(path);
        assert_eq!(result, None, "expected None for nonexistent path");
    }

    #[test]
    fn with_suppressed_stderr_returns_value() {
        let result = with_suppressed_stderr(|| 42_u32);
        assert_eq!(result, 42, "closure return value should be forwarded");
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
