#!/bin/bash
# Launch a nested GNOME Shell for extension development.
# The nested shell runs inside a window — your real session is unaffected.
# Close the window (or Ctrl+C) to stop, edit code, relaunch. ~2-3s cycle.
#
# Usage:
#   ./gnome/dev.sh              # normal
#   ./gnome/dev.sh --verbose    # all GLib/Shell debug messages

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXT_DIR="$SCRIPT_DIR/voicsh@voic.sh"
EXT_UUID="voicsh@voic.sh"
INSTALL_DIR="$HOME/.local/share/gnome-shell/extensions/$EXT_UUID"

# --- Preflight checks ---

if ! command -v gnome-shell &>/dev/null; then
    echo "Error: gnome-shell not found. Install gnome-shell to develop the extension." >&2
    exit 1
fi

if ! command -v glib-compile-schemas &>/dev/null; then
    echo "Error: glib-compile-schemas not found. Install libglib2.0-dev (Debian) or glib2-devel (Fedora)." >&2
    exit 1
fi

GNOME_MAJOR="$(gnome-shell --version | awk '{print int($3)}')"

if [ "$GNOME_MAJOR" -ge 49 ] && [ ! -x /usr/libexec/mutter-devkit ]; then
    echo "Error: mutter-devkit not found. GNOME $GNOME_MAJOR requires it for nested shell." >&2
    echo "  sudo apt install mutter-dev-bin   # Debian/Ubuntu" >&2
    echo "  sudo dnf install mutter-devel     # Fedora" >&2
    exit 1
fi

# --- Symlink extension source into GNOME's extension directory ---

if [ -L "$INSTALL_DIR" ]; then
    # Already a symlink — verify it points to our source
    current_target="$(readlink -f "$INSTALL_DIR")"
    expected_target="$(readlink -f "$EXT_DIR")"
    if [ "$current_target" != "$expected_target" ]; then
        echo "Warning: $INSTALL_DIR points to $current_target, relinking to $expected_target" >&2
        rm "$INSTALL_DIR"
        ln -s "$EXT_DIR" "$INSTALL_DIR"
    fi
elif [ -e "$INSTALL_DIR" ]; then
    BACKUP="/tmp/$EXT_UUID.dev-backup"
    echo "Existing install found at $INSTALL_DIR (not a symlink)."
    echo "Moving to $BACKUP and replacing with dev symlink."
    rm -rf "$BACKUP"
    mv "$INSTALL_DIR" "$BACKUP"
    ln -s "$EXT_DIR" "$INSTALL_DIR"
    echo "Symlinked $EXT_DIR -> $INSTALL_DIR (backup at $BACKUP)"
else
    mkdir -p "$(dirname "$INSTALL_DIR")"
    ln -s "$EXT_DIR" "$INSTALL_DIR"
    echo "Symlinked $EXT_DIR -> $INSTALL_DIR"
fi

# --- Compile GSettings schema ---

SCHEMA_DIR="$EXT_DIR/schemas"
if [ -d "$SCHEMA_DIR" ]; then
    glib-compile-schemas "$SCHEMA_DIR"
fi

# --- Debug flags ---

if [[ "${1:-}" == "--verbose" ]]; then
    export G_MESSAGES_DEBUG=all
    export SHELL_DEBUG=all
    echo "Debug output enabled (G_MESSAGES_DEBUG=all, SHELL_DEBUG=all)"
fi

# --- Detect GNOME Shell version for correct flag ---

if [ "$GNOME_MAJOR" -ge 49 ]; then
    SHELL_FLAG="--devkit"
else
    SHELL_FLAG="--nested"
fi

echo "Launching nested GNOME Shell ($SHELL_FLAG) with $EXT_UUID enabled..."
echo "Close the window or press Ctrl+C to stop."
echo ""

# --- Launch ---
# dbus-run-session gives the nested shell its own D-Bus so it doesn't
# conflict with the host session. We pre-enable the extension via gsettings
# so it's active immediately on launch.
# GNOME 49 devkit creates a marker file that disables extensions — remove it
# so our gsettings override takes effect cleanly on repeated runs.

exec dbus-run-session bash -c "
    rm -f /run/user/\$(id -u)/gnome-shell-disable-extensions && \
    gsettings set org.gnome.shell enabled-extensions \"['$EXT_UUID']\" && \
    gnome-shell $SHELL_FLAG --wayland
"
