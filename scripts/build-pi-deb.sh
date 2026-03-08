#!/bin/bash
# Build a Debian package for voicsh on Raspberry Pi (aarch64).
#
# Prerequisites:
#   rustup target add aarch64-unknown-linux-gnu
#   sudo apt install gcc-aarch64-linux-gnu
#
# Usage:
#   ./scripts/build-pi-deb.sh
#
# Output:
#   target/voicsh_<version>_arm64.deb

set -euo pipefail

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
ARCH="arm64"
TARGET="aarch64-unknown-linux-gnu"
PKG_NAME="voicsh"
DEB_DIR="target/${PKG_NAME}_${VERSION}_${ARCH}"

echo "Building voicsh ${VERSION} for ${TARGET}..."

# Cross-compile
cargo build --release --target "${TARGET}" --no-default-features --features pi

# Create package directory structure
rm -rf "${DEB_DIR}"
mkdir -p "${DEB_DIR}/DEBIAN"
mkdir -p "${DEB_DIR}/usr/local/bin"
mkdir -p "${DEB_DIR}/usr/local/lib/voicsh"
mkdir -p "${DEB_DIR}/etc/systemd/system"

# Binary
cp "target/${TARGET}/release/voicsh" "${DEB_DIR}/usr/local/bin/"

# Gadget setup script
cp scripts/voicsh-gadget-setup.sh "${DEB_DIR}/usr/local/lib/voicsh/"
chmod +x "${DEB_DIR}/usr/local/lib/voicsh/voicsh-gadget-setup.sh"

# Systemd units (adjust gadget script path)
sed 's|/usr/local/bin/voicsh-gadget-setup.sh|/usr/local/lib/voicsh/voicsh-gadget-setup.sh|' \
    scripts/voicsh-gadget.service > "${DEB_DIR}/etc/systemd/system/voicsh-gadget.service"
cp scripts/voicsh-pi.service "${DEB_DIR}/etc/systemd/system/voicsh-pi.service"

# DEBIAN/control
cat > "${DEB_DIR}/DEBIAN/control" << CTRL
Package: ${PKG_NAME}
Version: ${VERSION}
Architecture: ${ARCH}
Maintainer: voicsh <voicsh@users.noreply.github.com>
Description: Offline voice typing — Raspberry Pi USB keyboard mode
 voicsh transcribes speech offline using Whisper and types the result
 via USB HID keyboard gadget. Plug the Pi into any computer — no host
 software needed.
Depends: libc6, libasound2
Section: utils
Priority: optional
CTRL

# DEBIAN/postinst
cat > "${DEB_DIR}/DEBIAN/postinst" << 'POSTINST'
#!/bin/bash
set -e

# Create voicsh user if it doesn't exist
if ! id -u voicsh >/dev/null 2>&1; then
    useradd --system --no-create-home --shell /usr/sbin/nologin voicsh
fi

# Add voicsh user to required groups
usermod -aG gpio,audio voicsh 2>/dev/null || true

# Ensure dwc2 overlay and modules are configured
if ! grep -q 'dtoverlay=dwc2' /boot/config.txt 2>/dev/null; then
    echo "NOTE: Add 'dtoverlay=dwc2' to /boot/config.txt for USB gadget mode."
fi
if ! grep -q 'dwc2' /etc/modules 2>/dev/null; then
    echo "NOTE: Add 'dwc2' and 'libcomposite' to /etc/modules."
fi

# Reload systemd
systemctl daemon-reload

echo "voicsh installed. Enable with:"
echo "  sudo systemctl enable --now voicsh-gadget voicsh-pi"
POSTINST
chmod +x "${DEB_DIR}/DEBIAN/postinst"

# DEBIAN/prerm
cat > "${DEB_DIR}/DEBIAN/prerm" << 'PRERM'
#!/bin/bash
set -e
systemctl stop voicsh-pi 2>/dev/null || true
systemctl stop voicsh-gadget 2>/dev/null || true
systemctl disable voicsh-pi 2>/dev/null || true
systemctl disable voicsh-gadget 2>/dev/null || true
PRERM
chmod +x "${DEB_DIR}/DEBIAN/prerm"

# Build .deb
dpkg-deb --build "${DEB_DIR}"

DEB_FILE="target/${PKG_NAME}_${VERSION}_${ARCH}.deb"
echo ""
echo "Package built: ${DEB_FILE}"
echo "Size: $(du -h "${DEB_FILE}" | cut -f1)"
echo ""
echo "Install on Pi: sudo dpkg -i ${PKG_NAME}_${VERSION}_${ARCH}.deb"
