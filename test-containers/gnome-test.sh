#!/bin/bash
set -euo pipefail

# Test the voicsh GNOME extension in a headless GNOME Shell container.
# Uses ddterm/gnome-shell-image (Fedora 43, GNOME Shell 49).

IMAGE="ghcr.io/ddterm/gnome-shell-image/fedora-43"
EXT_DIR="$(cd "$(dirname "$0")/.." && pwd)/gnome/voicsh@voic.sh"
CID=""

cleanup() {
    [ -n "$CID" ] && docker rm -f "$CID" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "=== Pulling GNOME Shell container image ==="
docker pull "$IMAGE"

echo "=== Starting container with systemd ==="
CID=$(docker run -d --tty --privileged "$IMAGE")

echo "=== Waiting for systemd-logind ==="
for i in $(seq 1 30); do
    if docker exec "$CID" busctl status org.freedesktop.login1 >/dev/null 2>&1; then
        echo "  logind ready after ${i}s"
        break
    fi
    sleep 1
done

RUNTIME_DIR="/run/user/0"
ENV_ARGS=(
    "--env=XDG_RUNTIME_DIR=$RUNTIME_DIR"
    "--env=NO_AT_BRIDGE=1"
    "--env=GTK_A11Y=none"
)

echo "=== Installing voicsh extension (before Shell starts) ==="
EXT_DEST="/root/.local/share/gnome-shell/extensions/voicsh@voic.sh"
docker exec "$CID" mkdir -p "$EXT_DEST/schemas"
docker cp "$EXT_DIR/extension.js"   "$CID:$EXT_DEST/"
docker cp "$EXT_DIR/metadata.json"  "$CID:$EXT_DEST/"
docker cp "$EXT_DIR/stylesheet.css" "$CID:$EXT_DEST/"
docker cp "$EXT_DIR/schemas/."      "$CID:$EXT_DEST/schemas/"

echo "=== Compiling schemas ==="
docker exec "$CID" glib-compile-schemas "$EXT_DEST/schemas"

echo "=== Starting D-Bus session ==="
docker exec "${ENV_ARGS[@]}" "$CID" \
    mkdir -p "$RUNTIME_DIR"
docker exec "${ENV_ARGS[@]}" "$CID" \
    dbus-daemon --session --nopidfile --syslog --fork \
    "--address=unix:path=$RUNTIME_DIR/bus"

ENV_ARGS+=("--env=DBUS_SESSION_BUS_ADDRESS=unix:path=$RUNTIME_DIR/bus")

echo "=== Enabling extension via gsettings ==="
docker exec "${ENV_ARGS[@]}" "$CID" \
    gsettings set org.gnome.shell enabled-extensions "['voicsh@voic.sh']"
docker exec "${ENV_ARGS[@]}" "$CID" \
    gsettings set org.gnome.shell disable-user-extensions false

echo "=== Starting headless GNOME Shell ==="
docker exec -d "${ENV_ARGS[@]}" "$CID" \
    gnome-shell --wayland --headless --unsafe-mode --virtual-monitor 1600x960

echo "=== Waiting for GNOME Shell to be ready ==="
for i in $(seq 1 60); do
    if docker exec "${ENV_ARGS[@]}" "$CID" \
        gdbus call --session --dest=org.gnome.Shell --object-path=/org/gnome/Shell \
        --method=org.gnome.Shell.Eval 'Main.layoutManager._startingUp' 2>/dev/null \
        | grep -q '"false"'; then
        echo "  GNOME Shell ready after ${i}s"
        break
    fi
    sleep 1
done

sleep 2

echo "=== Checking extension status ==="
docker exec "${ENV_ARGS[@]}" "$CID" \
    gnome-extensions info voicsh@voic.sh 2>&1 || true

echo ""
echo "=== Checking for extension errors in journal ==="
ERRORS=$(docker exec "$CID" journalctl --no-pager -b 2>/dev/null \
    | grep -iE 'voicsh|error.*extension|critical' \
    | grep -v 'error_quark' \
    | grep -v 'No error' || true)

if [ -n "$ERRORS" ]; then
    echo "$ERRORS"
else
    echo "  No errors found"
fi

echo ""
echo "=== Taking screenshot ==="
SCREENSHOT_RESULT=$(docker exec "${ENV_ARGS[@]}" "$CID" \
    gdbus call --session \
    --dest=org.gnome.Shell.Screenshot \
    --object-path=/org/gnome/Shell/Screenshot \
    --method=org.gnome.Shell.Screenshot.Screenshot \
    true false '/tmp/screenshot.png' 2>&1 || true)
echo "  $SCREENSHOT_RESULT"

docker cp "$CID:/tmp/screenshot.png" "$(dirname "$0")/gnome-screenshot.png" 2>/dev/null \
    && echo "  Saved to test-containers/gnome-screenshot.png" || true

echo ""
echo "=== Extension list ==="
docker exec "${ENV_ARGS[@]}" "$CID" \
    gnome-extensions list 2>&1 || true

echo ""
echo "=== Done ==="
