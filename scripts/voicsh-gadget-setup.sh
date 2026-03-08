#!/bin/bash
# voicsh USB Composite Gadget Setup for Raspberry Pi
#
# Sets up a USB HID keyboard + Mass Storage gadget via Linux configfs.
# Run as root, typically via systemd (voicsh-gadget.service).
#
# Prerequisites:
#   /boot/config.txt: dtoverlay=dwc2
#   /etc/modules: dwc2, libcomposite

set -euo pipefail

GADGET=/sys/kernel/config/usb_gadgets/voicsh
CONFIG_IMG=/var/lib/voicsh/config.img

# ── Create config image if it doesn't exist ───────────────────────────
if [ ! -f "$CONFIG_IMG" ]; then
    echo "Creating config image at $CONFIG_IMG..."
    mkdir -p "$(dirname "$CONFIG_IMG")"
    dd if=/dev/zero of="$CONFIG_IMG" bs=1M count=4 2>/dev/null
    mkfs.vfat -n VOICSH "$CONFIG_IMG"

    # Mount and create default config
    MOUNT_DIR=$(mktemp -d)
    mount -o loop "$CONFIG_IMG" "$MOUNT_DIR"
    cat > "$MOUNT_DIR/config.toml" << 'TOML'
# voicsh configuration — edit and save, changes apply automatically.
# See: https://github.com/burka/voicsh

[stt]
model = "small"
language = "auto"

[injection]
backend = "usb-hid"
layout = "us"
TOML
    umount "$MOUNT_DIR"
    rmdir "$MOUNT_DIR"
    echo "Config image created with default config.toml"
fi

# ── Tear down existing gadget if present ──────────────────────────────
if [ -d "$GADGET" ]; then
    echo "Removing existing gadget..."
    echo "" > "$GADGET/UDC" 2>/dev/null || true
    rm -f "$GADGET/configs/c.1/hid.usb0" 2>/dev/null || true
    rm -f "$GADGET/configs/c.1/mass_storage.usb0" 2>/dev/null || true
    rmdir "$GADGET/configs/c.1/strings/0x409" 2>/dev/null || true
    rmdir "$GADGET/configs/c.1" 2>/dev/null || true
    rmdir "$GADGET/functions/hid.usb0" 2>/dev/null || true
    rmdir "$GADGET/functions/mass_storage.usb0" 2>/dev/null || true
    rmdir "$GADGET/strings/0x409" 2>/dev/null || true
    rmdir "$GADGET" 2>/dev/null || true
fi

# ── Create gadget ─────────────────────────────────────────────────────
echo "Setting up USB composite gadget..."

mkdir -p "$GADGET"
echo 0x1d6b > "$GADGET/idVendor"   # Linux Foundation
echo 0x0104 > "$GADGET/idProduct"  # Multifunction Composite Gadget
echo 0x0100 > "$GADGET/bcdDevice"
echo 0x0200 > "$GADGET/bcdUSB"

mkdir -p "$GADGET/strings/0x409"
echo "voicsh"               > "$GADGET/strings/0x409/manufacturer"
echo "voicsh Voice Keyboard" > "$GADGET/strings/0x409/product"

# ── HID Keyboard function ────────────────────────────────────────────
mkdir -p "$GADGET/functions/hid.usb0"
echo 1 > "$GADGET/functions/hid.usb0/protocol"       # Keyboard
echo 1 > "$GADGET/functions/hid.usb0/subclass"        # Boot Interface
echo 8 > "$GADGET/functions/hid.usb0/report_length"   # 8-byte reports

# HID Report Descriptor for a standard keyboard
echo -ne '\x05\x01\x09\x06\xa1\x01\x05\x07\x19\xe0\x29\xe7\x15\x00\x25\x01\x75\x01\x95\x08\x81\x02\x95\x01\x75\x08\x81\x01\x95\x05\x75\x01\x05\x08\x19\x01\x29\x05\x91\x02\x95\x01\x75\x03\x91\x01\x95\x06\x75\x08\x15\x00\x26\xff\x00\x05\x07\x19\x00\x29\xff\x81\x00\xc0' \
    > "$GADGET/functions/hid.usb0/report_desc"

# ── Mass Storage function (config USB stick) ─────────────────────────
mkdir -p "$GADGET/functions/mass_storage.usb0"
echo 1            > "$GADGET/functions/mass_storage.usb0/stall"
echo 0            > "$GADGET/functions/mass_storage.usb0/lun.0/cdrom"
echo 0            > "$GADGET/functions/mass_storage.usb0/lun.0/ro"
echo 0            > "$GADGET/functions/mass_storage.usb0/lun.0/nofua"
echo "$CONFIG_IMG" > "$GADGET/functions/mass_storage.usb0/lun.0/file"

# ── Activate gadget ──────────────────────────────────────────────────
mkdir -p "$GADGET/configs/c.1/strings/0x409"
echo "Keyboard + Config" > "$GADGET/configs/c.1/strings/0x409/configuration"
echo 250                 > "$GADGET/configs/c.1/MaxPower"

ln -sf "$GADGET/functions/hid.usb0"          "$GADGET/configs/c.1/"
ln -sf "$GADGET/functions/mass_storage.usb0" "$GADGET/configs/c.1/"

# Bind to UDC (USB Device Controller)
UDC=$(ls /sys/class/udc | head -1)
if [ -z "$UDC" ]; then
    echo "ERROR: No USB Device Controller found." >&2
    echo "Ensure dtoverlay=dwc2 is in /boot/config.txt and dwc2 module is loaded." >&2
    exit 1
fi
echo "$UDC" > "$GADGET/UDC"

echo "USB gadget active: HID keyboard + config storage on $UDC"
