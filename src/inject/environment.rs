//! Desktop environment detection and backend recommendation.
//!
//! Detects the current Wayland desktop environment and recommends
//! the best text injection backend based on compositor capabilities.

use crate::config::InjectionBackend;
use crate::diagnostics::{
    CheckResult, check_command, check_portal, check_wtype, check_ydotool_backend,
};

/// Known desktop environments with different injection support.
#[derive(Debug, Clone, PartialEq)]
pub enum DesktopEnvironment {
    Gnome,
    Kde,
    Sway,
    Hyprland,
    Wlroots,
    Unknown(String),
}

impl std::fmt::Display for DesktopEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gnome => write!(f, "GNOME"),
            Self::Kde => write!(f, "KDE Plasma"),
            Self::Sway => write!(f, "Sway"),
            Self::Hyprland => write!(f, "Hyprland"),
            Self::Wlroots => write!(f, "wlroots-based"),
            Self::Unknown(name) => {
                if name.is_empty() {
                    write!(f, "Unknown")
                } else {
                    write!(f, "{}", name)
                }
            }
        }
    }
}

/// Result of environment detection with tool availability and recommendation.
#[derive(Debug, Clone)]
pub struct DetectedEnvironment {
    pub desktop: DesktopEnvironment,
    pub portal_available: bool,
    pub wtype_available: bool,
    pub ydotool_available: bool,
    pub wl_copy_available: bool,
    pub recommended_backend: InjectionBackend,
}

/// Detect the current desktop environment and available injection tools.
pub fn detect_environment() -> DetectedEnvironment {
    let desktop = detect_desktop();

    let portal_available = check_portal() == CheckResult::Ok;
    let wtype_available = check_wtype() == CheckResult::Ok;
    let ydotool_available = {
        let cmd = check_command("ydotool");
        cmd == CheckResult::Ok && check_ydotool_backend() == CheckResult::Ok
    };
    let wl_copy_available = check_command("wl-copy") == CheckResult::Ok;

    let recommended_backend = recommend_backend(
        &desktop,
        portal_available,
        wtype_available,
        ydotool_available,
    );

    DetectedEnvironment {
        desktop,
        portal_available,
        wtype_available,
        ydotool_available,
        wl_copy_available,
        recommended_backend,
    }
}

/// Detect desktop environment from XDG_CURRENT_DESKTOP and XDG_SESSION_DESKTOP.
fn detect_desktop() -> DesktopEnvironment {
    let xdg_current = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_lowercase();
    let xdg_session = std::env::var("XDG_SESSION_DESKTOP")
        .unwrap_or_default()
        .to_lowercase();

    let combined = format!("{} {}", xdg_current, xdg_session);

    if combined.contains("gnome") || combined.contains("ubuntu") {
        DesktopEnvironment::Gnome
    } else if combined.contains("kde") || combined.contains("plasma") {
        DesktopEnvironment::Kde
    } else if combined.contains("sway") {
        DesktopEnvironment::Sway
    } else if combined.contains("hyprland") {
        DesktopEnvironment::Hyprland
    } else if combined.contains("wlroots") {
        DesktopEnvironment::Wlroots
    } else {
        DesktopEnvironment::Unknown(xdg_current)
    }
}

/// Recommend the best backend for the detected environment.
///
/// | Environment   | Best Backend | Reason                           |
/// |---------------|-------------|----------------------------------|
/// | GNOME         | Portal      | wtype doesn't work on Mutter     |
/// | KDE           | Portal/wtype| Both work                        |
/// | Sway/wlroots  | wtype       | Simple, no daemon                |
/// | Hyprland      | wtype       | Simple, no daemon                |
/// | Unknown       | wtype→ydotool| Safe fallback                   |
fn recommend_backend(
    desktop: &DesktopEnvironment,
    portal_available: bool,
    wtype_available: bool,
    ydotool_available: bool,
) -> InjectionBackend {
    match desktop {
        DesktopEnvironment::Gnome => {
            if portal_available {
                InjectionBackend::Portal
            } else if ydotool_available {
                InjectionBackend::Ydotool
            } else {
                // Portal is needed on GNOME but not available — still recommend it
                // so the user gets a clear error about what to install
                InjectionBackend::Portal
            }
        }
        DesktopEnvironment::Kde => {
            if wtype_available {
                InjectionBackend::Wtype
            } else if portal_available {
                InjectionBackend::Portal
            } else if ydotool_available {
                InjectionBackend::Ydotool
            } else {
                InjectionBackend::Wtype
            }
        }
        DesktopEnvironment::Sway | DesktopEnvironment::Hyprland | DesktopEnvironment::Wlroots => {
            if wtype_available {
                InjectionBackend::Wtype
            } else if ydotool_available {
                InjectionBackend::Ydotool
            } else {
                InjectionBackend::Wtype
            }
        }
        DesktopEnvironment::Unknown(_) => {
            if wtype_available {
                InjectionBackend::Wtype
            } else if ydotool_available {
                InjectionBackend::Ydotool
            } else if portal_available {
                InjectionBackend::Portal
            } else {
                InjectionBackend::Wtype
            }
        }
    }
}

/// Print a human-readable environment summary to stderr.
pub fn print_environment_summary(env: &DetectedEnvironment) {
    eprintln!("Environment: {} (Wayland)", env.desktop);

    let status = |available: bool, note: &str| -> String {
        if available {
            "available".to_string()
        } else if note.is_empty() {
            "not found".to_string()
        } else {
            format!("not found ({})", note)
        }
    };

    let wtype_note = match env.desktop {
        DesktopEnvironment::Gnome => "not supported on GNOME/Mutter",
        _ => "",
    };

    eprintln!(
        "  Portal (RemoteDesktop): {}",
        status(env.portal_available, "")
    );
    eprintln!(
        "  wtype:                  {}",
        status(env.wtype_available, wtype_note)
    );
    eprintln!(
        "  ydotool:                {}",
        status(env.ydotool_available, "")
    );
    eprintln!(
        "  wl-copy:                {}",
        status(env.wl_copy_available, "")
    );
    eprintln!();
    eprintln!("Recommended backend: {:?}", env.recommended_backend);

    // Print explanatory note based on recommendation
    match env.recommended_backend {
        InjectionBackend::Portal => {
            eprintln!(
                "  Note: {} uses the \"Remote Desktop\" portal for keyboard access.",
                env.desktop
            );
            eprintln!("  You may see a system dialog on first use — this is normal.");
            eprintln!("  voicsh only simulates keyboard input, not screen sharing.");
        }
        InjectionBackend::Wtype => {
            eprintln!("  wtype provides direct keyboard simulation — no daemon needed.");
        }
        InjectionBackend::Ydotool => {
            eprintln!("  ydotool requires the ydotoold daemon to be running.");
        }
        InjectionBackend::Auto => {
            eprintln!("  Will try available backends at runtime.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desktop_display_gnome() {
        assert_eq!(DesktopEnvironment::Gnome.to_string(), "GNOME");
    }

    #[test]
    fn test_desktop_display_kde() {
        assert_eq!(DesktopEnvironment::Kde.to_string(), "KDE Plasma");
    }

    #[test]
    fn test_desktop_display_sway() {
        assert_eq!(DesktopEnvironment::Sway.to_string(), "Sway");
    }

    #[test]
    fn test_desktop_display_hyprland() {
        assert_eq!(DesktopEnvironment::Hyprland.to_string(), "Hyprland");
    }

    #[test]
    fn test_desktop_display_wlroots() {
        assert_eq!(DesktopEnvironment::Wlroots.to_string(), "wlroots-based");
    }

    #[test]
    fn test_desktop_display_unknown_empty() {
        assert_eq!(
            DesktopEnvironment::Unknown("".to_string()).to_string(),
            "Unknown"
        );
    }

    #[test]
    fn test_desktop_display_unknown_named() {
        assert_eq!(
            DesktopEnvironment::Unknown("cosmic".to_string()).to_string(),
            "cosmic"
        );
    }

    #[test]
    fn test_recommend_gnome_with_portal() {
        let backend = recommend_backend(&DesktopEnvironment::Gnome, true, false, false);
        assert_eq!(backend, InjectionBackend::Portal);
    }

    #[test]
    fn test_recommend_gnome_with_ydotool_fallback() {
        let backend = recommend_backend(&DesktopEnvironment::Gnome, false, false, true);
        assert_eq!(backend, InjectionBackend::Ydotool);
    }

    #[test]
    fn test_recommend_gnome_nothing_available() {
        let backend = recommend_backend(&DesktopEnvironment::Gnome, false, false, false);
        assert_eq!(backend, InjectionBackend::Portal);
    }

    #[test]
    fn test_recommend_sway_with_wtype() {
        let backend = recommend_backend(&DesktopEnvironment::Sway, false, true, false);
        assert_eq!(backend, InjectionBackend::Wtype);
    }

    #[test]
    fn test_recommend_sway_with_ydotool_fallback() {
        let backend = recommend_backend(&DesktopEnvironment::Sway, false, false, true);
        assert_eq!(backend, InjectionBackend::Ydotool);
    }

    #[test]
    fn test_recommend_sway_nothing_available() {
        let backend = recommend_backend(&DesktopEnvironment::Sway, false, false, false);
        assert_eq!(backend, InjectionBackend::Wtype);
    }

    #[test]
    fn test_recommend_kde_prefers_wtype() {
        let backend = recommend_backend(&DesktopEnvironment::Kde, true, true, true);
        assert_eq!(backend, InjectionBackend::Wtype);
    }

    #[test]
    fn test_recommend_kde_portal_fallback() {
        let backend = recommend_backend(&DesktopEnvironment::Kde, true, false, false);
        assert_eq!(backend, InjectionBackend::Portal);
    }

    #[test]
    fn test_recommend_unknown_prefers_wtype() {
        let backend = recommend_backend(
            &DesktopEnvironment::Unknown("".to_string()),
            false,
            true,
            true,
        );
        assert_eq!(backend, InjectionBackend::Wtype);
    }

    #[test]
    fn test_recommend_unknown_ydotool_fallback() {
        let backend = recommend_backend(
            &DesktopEnvironment::Unknown("".to_string()),
            false,
            false,
            true,
        );
        assert_eq!(backend, InjectionBackend::Ydotool);
    }

    #[test]
    fn test_recommend_unknown_portal_fallback() {
        let backend = recommend_backend(
            &DesktopEnvironment::Unknown("".to_string()),
            true,
            false,
            false,
        );
        assert_eq!(backend, InjectionBackend::Portal);
    }

    #[test]
    fn test_recommend_hyprland_same_as_sway() {
        let sway = recommend_backend(&DesktopEnvironment::Sway, true, true, true);
        let hypr = recommend_backend(&DesktopEnvironment::Hyprland, true, true, true);
        assert_eq!(sway, hypr);
    }

    #[test]
    fn test_detected_environment_fields() {
        let env = DetectedEnvironment {
            desktop: DesktopEnvironment::Gnome,
            portal_available: true,
            wtype_available: false,
            ydotool_available: false,
            wl_copy_available: true,
            recommended_backend: InjectionBackend::Portal,
        };
        assert!(env.portal_available);
        assert!(!env.wtype_available);
        assert!(env.wl_copy_available);
        assert_eq!(env.recommended_backend, InjectionBackend::Portal);
    }
}
