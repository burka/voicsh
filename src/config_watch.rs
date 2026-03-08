//! Config watcher for USB Mass Storage FAT32 image.
//!
//! Polls a FAT32 disk image (used by the USB gadget mass storage function)
//! for changes to `config.toml`. When the host edits the file via the
//! virtual USB drive, this module detects the change and reloads the config.
//!
//! Uses the `fatfs` crate to read the FAT32 image directly — no mount needed.

use crate::config::Config;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Default path to the USB mass storage config image.
pub const DEFAULT_CONFIG_IMAGE: &str = "/var/lib/voicsh/config.img";

/// Default poll interval in seconds.
const POLL_INTERVAL_SECS: u64 = 5;

/// Config file name inside the FAT32 image.
const CONFIG_FILENAME: &str = "config.toml";

/// Read `config.toml` from a FAT32 disk image file.
///
/// Opens the image read-only and extracts the config file contents.
/// Returns `None` if the file doesn't exist inside the image.
pub fn read_config_from_image(image_path: &Path) -> Result<Option<String>, String> {
    let file = std::fs::File::open(image_path)
        .map_err(|e| format!("Failed to open config image {}: {e}", image_path.display()))?;

    let fs = fatfs::FileSystem::new(file, fatfs::FsOptions::new()).map_err(|e| {
        format!(
            "Failed to read FAT32 filesystem from {}: {e}",
            image_path.display()
        )
    })?;

    let root = fs.root_dir();
    let mut entry = match root.open_file(CONFIG_FILENAME) {
        Ok(f) => f,
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Failed to open {CONFIG_FILENAME} in image: {e}")),
    };

    let mut contents = String::new();
    entry
        .read_to_string(&mut contents)
        .map_err(|e| format!("Failed to read {CONFIG_FILENAME} from image: {e}"))?;

    Ok(Some(contents))
}

/// Parse a config TOML string into a `Config`.
fn parse_config(toml_str: &str) -> Result<Config, String> {
    toml::from_str(toml_str).map_err(|e| format!("Failed to parse config.toml: {e}"))
}

/// Get the modification time of a file, or `None` if unavailable.
fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Run the config watcher loop.
///
/// Polls the FAT32 image for changes every `POLL_INTERVAL_SECS` seconds.
/// When a change is detected, reads config.toml from the image and calls
/// the provided callback with the new config.
///
/// This function blocks forever and is intended to run in a dedicated thread.
pub fn run_config_watcher<F>(image_path: PathBuf, mut on_config_changed: F)
where
    F: FnMut(Config),
{
    let mut last_mtime: Option<SystemTime> = None;
    let mut last_config_hash: Option<u64> = None;

    loop {
        std::thread::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS));

        // Check if image file mtime changed
        let current_mtime = file_mtime(&image_path);
        if current_mtime == last_mtime {
            continue;
        }
        last_mtime = current_mtime;

        // Read config from image
        let toml_str = match read_config_from_image(&image_path) {
            Ok(Some(s)) => s,
            Ok(None) => continue, // no config.toml in image
            Err(e) => {
                eprintln!("voicsh: config watcher: {e}");
                continue;
            }
        };

        // Quick hash check to avoid applying identical configs
        let hash = simple_hash(&toml_str);
        if last_config_hash == Some(hash) {
            continue;
        }
        last_config_hash = Some(hash);

        // Parse and apply
        match parse_config(&toml_str) {
            Ok(config) => {
                eprintln!("voicsh: config watcher: config.toml changed, reloading");
                on_config_changed(config);
            }
            Err(e) => {
                eprintln!("voicsh: config watcher: {e}");
            }
        }
    }
}

/// Simple non-cryptographic hash for change detection.
fn simple_hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Create a FAT32 image in a temp file with the given files.
    fn create_test_image(files: &[(&str, &str)]) -> tempfile::NamedTempFile {
        let tmp = tempfile::NamedTempFile::new().expect("create temp file");

        // Create a 1MB FAT image
        let image_size: u64 = 1024 * 1024;
        tmp.as_file().set_len(image_size).expect("set file length");

        // Format as FAT directly on the temp file
        {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(tmp.path())
                .expect("open for formatting");
            let opts = fatfs::FormatVolumeOptions::new();
            fatfs::format_volume(file, opts).expect("format FAT volume");
        }

        // Open the FAT filesystem and write files
        {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(tmp.path())
                .expect("open for writing");
            let fs =
                fatfs::FileSystem::new(file, fatfs::FsOptions::new()).expect("open FAT filesystem");
            let root = fs.root_dir();

            for (name, content) in files {
                let mut entry = root.create_file(name).expect("create file in image");
                entry
                    .write_all(content.as_bytes())
                    .expect("write file content");
            }
        }

        tmp
    }

    #[test]
    fn read_config_from_image_with_valid_toml() {
        let toml_content = "[stt]\nmodel = \"small\"\nlanguage = \"de\"\n";
        let image = create_test_image(&[("config.toml", toml_content)]);

        let result = read_config_from_image(image.path());
        let contents = result
            .expect("should succeed")
            .expect("should find config.toml");
        assert_eq!(contents, toml_content);
    }

    #[test]
    fn read_config_from_image_missing_file() {
        let image = create_test_image(&[("other.txt", "hello")]);

        let result = read_config_from_image(image.path());
        let contents = result.expect("should succeed");
        assert!(
            contents.is_none(),
            "Should return None when config.toml is missing"
        );
    }

    #[test]
    fn read_config_from_image_nonexistent_path() {
        let result = read_config_from_image(Path::new("/nonexistent/image.img"));
        assert!(result.is_err(), "Should fail for nonexistent path");
        let err = result.unwrap_err();
        assert!(
            err.contains("Failed to open"),
            "Error should mention open failure: {err}"
        );
    }

    #[test]
    fn parse_config_valid_toml() {
        let toml_str = "[stt]\nmodel = \"small\"\nlanguage = \"de\"\n";
        let config = parse_config(toml_str).expect("should parse");
        assert_eq!(config.stt.model, "small");
        assert_eq!(config.stt.language, "de");
    }

    #[test]
    fn parse_config_invalid_toml() {
        let result = parse_config("not valid { toml");
        assert!(result.is_err(), "Should fail for invalid TOML");
        let err = result.unwrap_err();
        assert!(
            err.contains("Failed to parse"),
            "Error should mention parse failure: {err}"
        );
    }

    #[test]
    fn parse_config_partial_overrides_use_defaults() {
        let toml_str = "[stt]\nmodel = \"tiny\"\n";
        let config = parse_config(toml_str).expect("should parse");
        assert_eq!(config.stt.model, "tiny");
        // Language should be the default "auto"
        assert_eq!(config.stt.language, crate::defaults::DEFAULT_LANGUAGE);
    }

    #[test]
    fn read_and_parse_roundtrip() {
        let toml_content = concat!(
            "[stt]\n",
            "model = \"base.en\"\n",
            "language = \"en\"\n",
            "\n",
            "[injection]\n",
            "backend = \"usb_hid\"\n",
            "layout = \"de\"\n",
        );
        let image = create_test_image(&[("config.toml", toml_content)]);

        let contents = read_config_from_image(image.path())
            .expect("should read")
            .expect("should find file");
        let config = parse_config(&contents).expect("should parse");

        assert_eq!(config.stt.model, "base.en");
        assert_eq!(config.stt.language, "en");
        assert_eq!(config.injection.layout, "de");
    }

    #[test]
    fn simple_hash_deterministic() {
        let a = simple_hash("hello world");
        let b = simple_hash("hello world");
        assert_eq!(a, b, "Same input should produce same hash");
    }

    #[test]
    fn simple_hash_different_inputs() {
        let a = simple_hash("hello");
        let b = simple_hash("world");
        assert_ne!(a, b, "Different inputs should produce different hashes");
    }

    #[test]
    fn file_mtime_returns_some_for_existing_file() {
        let tmp = tempfile::NamedTempFile::new().expect("create temp file");
        let mtime = file_mtime(tmp.path());
        assert!(mtime.is_some(), "Should return Some for existing file");
    }

    #[test]
    fn file_mtime_returns_none_for_missing_file() {
        let mtime = file_mtime(Path::new("/nonexistent/file"));
        assert!(mtime.is_none(), "Should return None for missing file");
    }
}
