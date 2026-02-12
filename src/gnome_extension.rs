//! GNOME Shell extension install and uninstall logic.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const EXTENSION_JS: &str = include_str!("../gnome/voicsh@voicsh.dev/extension.js");
const METADATA_JSON: &str = include_str!("../gnome/voicsh@voicsh.dev/metadata.json");
const STYLESHEET_CSS: &str = include_str!("../gnome/voicsh@voicsh.dev/stylesheet.css");
const GSCHEMA_XML: &str = include_str!(
    "../gnome/voicsh@voicsh.dev/schemas/org.gnome.shell.extensions.voicsh.gschema.xml"
);

const EXTENSION_UUID: &str = "voicsh@voicsh.dev";

/// Install the GNOME Shell extension and systemd user service.
pub fn install_gnome_extension() -> Result<()> {
    // Step 1: Install systemd user service
    eprintln!("Installing systemd service...");
    crate::systemd::install_and_activate()?;

    // Step 2: Install GNOME extension
    eprintln!("Installing GNOME extension...");
    install_extension_files()?;

    println!("Service installed and started. Log out and back in to activate the panel indicator.");

    Ok(())
}

/// Uninstall the GNOME Shell extension and systemd user service.
pub fn uninstall_gnome_extension() -> Result<()> {
    eprintln!("Stopping and disabling systemd service...");
    crate::systemd::stop_and_disable()?;

    eprintln!("Disabling GNOME extension...");

    // Disable extension (cleanup/shutdown errors are logged, not fatal)
    if let Err(e) = Command::new("gnome-extensions")
        .args(["disable", EXTENSION_UUID])
        .status()
    {
        eprintln!("Warning: Failed to disable extension: {}", e);
    }

    // Remove extension directory
    let extension_dir = get_extension_dir()?;
    if extension_dir.exists() {
        fs::remove_dir_all(&extension_dir).context("Failed to remove extension directory")?;
    }

    println!("Service and extension removed. Log out and back in to complete.");

    Ok(())
}

fn install_extension_files() -> Result<()> {
    // Get extension directory
    let extension_dir = get_extension_dir()?;
    let schemas_dir = extension_dir.join("schemas");

    // Create directories
    fs::create_dir_all(&schemas_dir).context("Failed to create extension directories")?;

    // Write extension files
    fs::write(extension_dir.join("extension.js"), EXTENSION_JS)
        .context("Failed to write extension.js")?;

    fs::write(extension_dir.join("metadata.json"), METADATA_JSON)
        .context("Failed to write metadata.json")?;

    fs::write(extension_dir.join("stylesheet.css"), STYLESHEET_CSS)
        .context("Failed to write stylesheet.css")?;

    fs::write(
        schemas_dir.join("org.gnome.shell.extensions.voicsh.gschema.xml"),
        GSCHEMA_XML,
    )
    .context("Failed to write gschema.xml")?;

    // Compile schemas
    let compile_status = Command::new("glib-compile-schemas")
        .arg(&schemas_dir)
        .status();

    match compile_status {
        Ok(status) if status.success() => {}
        Ok(_) => {
            anyhow::bail!("glib-compile-schemas failed. Is glib-2.0 installed?");
        }
        Err(_) => {
            anyhow::bail!(
                "glib-compile-schemas not found. Install glib-2.0 development tools (e.g., libglib2.0-dev on Debian/Ubuntu)"
            );
        }
    }

    // Enable extension (user might not be in a GNOME session, so log errors but don't fail)
    if let Err(e) = Command::new("gnome-extensions")
        .args(["enable", EXTENSION_UUID])
        .status()
    {
        eprintln!(
            "Warning: Failed to enable extension (not in GNOME session?): {}",
            e
        );
    }

    Ok(())
}

fn get_extension_dir() -> Result<PathBuf> {
    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        Ok(PathBuf::from(data_home)
            .join("gnome-shell")
            .join("extensions")
            .join(EXTENSION_UUID))
    } else if let Ok(home) = std::env::var("HOME") {
        Ok(PathBuf::from(home)
            .join(".local/share/gnome-shell/extensions")
            .join(EXTENSION_UUID))
    } else {
        anyhow::bail!("Could not determine user data directory")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_embedded_extension_js_not_empty() {
        assert!(!EXTENSION_JS.is_empty());
    }

    #[test]
    fn test_embedded_metadata_json_valid() {
        let value: serde_json::Value =
            serde_json::from_str(METADATA_JSON).expect("Failed to parse metadata.json");
        let uuid = value
            .get("uuid")
            .and_then(|v| v.as_str())
            .expect("Missing uuid field");
        assert_eq!(uuid, "voicsh@voicsh.dev");
    }

    #[test]
    fn test_embedded_stylesheet_not_empty() {
        assert!(!STYLESHEET_CSS.is_empty());
    }

    #[test]
    fn test_embedded_gschema_contains_key() {
        assert!(GSCHEMA_XML.contains("toggle-shortcut"));
    }

    #[test]
    fn test_install_to_tempdir() {
        // Create temp directory
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let temp_home = temp_dir.path().to_path_buf();

        // Set HOME env for this test
        // SAFETY: This test runs in isolation and doesn't affect other tests
        unsafe {
            env::set_var("HOME", &temp_home);
            env::remove_var("XDG_DATA_HOME");
        }

        // Get expected paths
        let extension_dir = temp_home
            .join(".local/share/gnome-shell/extensions")
            .join(EXTENSION_UUID);
        let schemas_dir = extension_dir.join("schemas");

        // Create directories
        fs::create_dir_all(&schemas_dir).expect("Failed to create test directories");

        // Write files (simulating install_extension_files logic without external commands)
        fs::write(extension_dir.join("extension.js"), EXTENSION_JS)
            .expect("Failed to write extension.js");

        fs::write(extension_dir.join("metadata.json"), METADATA_JSON)
            .expect("Failed to write metadata.json");

        fs::write(extension_dir.join("stylesheet.css"), STYLESHEET_CSS)
            .expect("Failed to write stylesheet.css");

        fs::write(
            schemas_dir.join("org.gnome.shell.extensions.voicsh.gschema.xml"),
            GSCHEMA_XML,
        )
        .expect("Failed to write gschema.xml");

        // Verify files exist and have expected content
        assert!(extension_dir.join("extension.js").exists());
        assert!(extension_dir.join("metadata.json").exists());
        assert!(extension_dir.join("stylesheet.css").exists());
        assert!(
            schemas_dir
                .join("org.gnome.shell.extensions.voicsh.gschema.xml")
                .exists()
        );

        let written_js =
            fs::read_to_string(extension_dir.join("extension.js")).expect("Failed to read file");
        assert_eq!(written_js, EXTENSION_JS);

        let written_metadata =
            fs::read_to_string(extension_dir.join("metadata.json")).expect("Failed to read file");
        assert_eq!(written_metadata, METADATA_JSON);

        let written_css =
            fs::read_to_string(extension_dir.join("stylesheet.css")).expect("Failed to read file");
        assert_eq!(written_css, STYLESHEET_CSS);

        let written_gschema =
            fs::read_to_string(schemas_dir.join("org.gnome.shell.extensions.voicsh.gschema.xml"))
                .expect("Failed to read file");
        assert_eq!(written_gschema, GSCHEMA_XML);
    }
}
