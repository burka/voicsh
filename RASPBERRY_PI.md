---
title: Raspberry Pi USB Keyboard
weight: 8
---

Raspberry Pi als USB-HID-Tastatur: Mikrofon + Knopf am Pi, Text wird per USB an den Host-Rechner getippt.

## Konzept

```
┌─────────────────────────────────────┐          USB-C/Micro-USB          ┌──────────────┐
│          Raspberry Pi               │ ──────────────────────────────── │  Host-Rechner │
│                                     │   (erscheint als USB-Tastatur    │  (beliebiges  │
│  [Mikrofon] → voicsh → USB HID     │    + USB-Stick fuer Config)      │   OS/Geraet)  │
│  [Knopf]   → Push-to-Talk          │                                  └──────────────┘
│  [LED]     → Status-Feedback       │
└─────────────────────────────────────┘
```

Der Pi laeuft headless. voicsh nimmt Audio auf, transkribiert offline, und sendet die Tastenanschlaege
per USB HID Gadget an den angeschlossenen Rechner. Der Host braucht keine Software — er sieht nur eine Tastatur.

## USB Composite Gadget

```
USB Composite Gadget:
  ├── HID Function       → /dev/hidg0   (Keyboard)
  └── Mass Storage       → config.img   (FAT32, ~4MB — Konfiguration)
```

**User Experience:**

1. Geraet einstecken → erscheint als USB-Tastatur **und** USB-Stick
2. Auf dem USB-Stick liegt `config.toml` — editieren, speichern, fertig
3. voicsh erkennt Aenderungen via inotify und laedt Config neu (kein Neustart)
4. Ein einzelner Knopf (GPIO): Push-to-Talk

## Board-Auswahl

### Performance-Vergleich

| | Pi Zero 2 W | Pi 4 (4GB) | **CM5 / Pi 5 (4GB)** | CM5 / Pi 5 (8GB) |
|---|---|---|---|---|
| **CPU** | 4x A53 @1GHz | 4x A72 @1.5GHz | **4x A76 @2.4GHz** | 4x A76 @2.4GHz |
| **tiny.en / 10s** | ~3s | ~1.5s | **~0.5s** | ~0.5s |
| **base.en / 10s** | ~8s | ~3s | **~1s** | ~1s |
| **small / 10s** | ~25s | ~10s | **~3-4s** | ~3-4s |
| **medium / 10s** | unmoeglich | ~30s+ | **~10s** | ~10s |
| **large** | nein | nein | eng (4GB) | **~25s** |
| **USB Gadget** | OTG nativ | dwc2 USB-C | **dwc2 USB-C** | dwc2 USB-C |

Quantisierte Modelle (q5_0) sind ~2x schneller bei minimalem Qualitaetsverlust.

### Form-Faktor-Vergleich

| | Pi 5 | **CM5 + CM5 MINIMA** | Pi Zero 2 W |
|---|---|---|---|
| **Groesse** | 85 x 56 mm | **61 x 61 mm** | 65 x 30 mm |
| **Gewicht** | ~45g | **~30g (geschaetzt)** | ~10g |
| **USB-A Ports** | 4x | **0** (USB-Mikro ueber Hub/I2S) | 0 |
| **USB-C** | 1x (Power/Gadget) | **2x (1x PD, 1x Data/OTG)** | 0 (Micro-USB) |
| **GPIO** | 40-pin Header | **I2C/SPI Header** | 40-pin Header |
| **Kuehlung** | Luefter/Alu-Case | **Alu-Case oder kleiner Kuehler** | passiv |
| **eMMC** | nein (microSD) | **optional 16/32/64GB** | nein (microSD) |
| **Preis (Board)** | ~65 EUR | **~45 EUR (CM5) + ~65 EUR (MINIMA)** | ~18 EUR |

### Empfehlung

**Fuer Prototyping: Raspberry Pi 5 (4GB) — ~65 EUR**
- Einfacher Einstieg, 4x USB-A fuer Mikrofon, viel Doku
- Alu-Passivkuehlgehaeuse erhaeltlich (~15 EUR)
- 85 x 56 mm — Kreditkartengroesse

**Fuer kompaktes Endgeraet: CM5 (4GB) + CM5 MINIMA — ~110 EUR**
- Gleicher BCM2712 Chip wie Pi 5, identische Performance
- 61 x 61 mm — deutlich kleiner
- USB-C OTG Port fuer Gadget-Modus
- eMMC statt microSD moeglich (schneller, zuverlaessiger)
- I2S-Mikrofon (INMP441) statt USB-Mikrofon (kein USB-A Port)
- Alu-Gehaeuse als Kuehlkoerper — komplett lautlos
- CM5 Modul: 55 x 40 mm, kleiner als eine Kreditkarte

**Nicht empfohlen fuer diesen Use-Case:**
- Orange Pi 5 / Radxa Rock 5C (RK3588S) — schnell, aber groesser, weniger Doku,
  USB Gadget-Modus nicht so gut dokumentiert wie bei Pi
- Pi Zero 2 W — zu langsam fuer brauchbare Whisper-Latenz

## Einkaufsliste

### Variante A: Pi 5 (Prototyping)

| Teil | Beispiel | ca. Preis |
|------|----------|-----------|
| Raspberry Pi 5 (4GB) | [offizieller Shop](https://www.raspberrypi.com/products/raspberry-pi-5/) | ~65 EUR |
| Alu-Passivkuehlgehaeuse | Geekworm P573, GeeekPi, KKSB oder 52Pi CNC Case | ~15 EUR |
| microSD-Karte (16GB+) | SanDisk Ultra 32GB | ~8 EUR |
| USB-C Datenkabel | Pi → Host | ~5 EUR |
| USB-Mikrofon | Mini-USB-Konferenzmikrofon | ~15 EUR |
| Taster + Jumperkabel | Push-to-Talk | ~2 EUR |

**Gesamt: ~110 EUR**

USB-Mikrofon geht an einen der 4x USB-A Ports, USB-C geht zum Host-Rechner. Kein Hub noetig.

### Variante B: CM5 + MINIMA (kompaktes Endgeraet)

| Teil | Beispiel | ca. Preis |
|------|----------|-----------|
| CM5 4GB (mit Wireless) | [CM5104032](https://www.raspberrypi.com/products/compute-module-5/) | ~55 EUR |
| CM5 MINIMA Carrier Board | [Seeed Studio](https://www.seeedstudio.com/CM5-MINIMA-p-6485.html) | ~65 EUR |
| I2S MEMS Mikrofon (INMP441) | kein USB-Port noetig, direkt an I2S/SPI Header | ~5 EUR |
| Taster + Jumperkabel | Push-to-Talk, an I2C/SPI Header | ~2 EUR |
| USB-C Datenkabel | MINIMA USB-C OTG → Host | ~5 EUR |

**Gesamt: ~132 EUR**

CM5 MINIMA hat 2x USB-C: einen fuer Power (PD), einen fuer Daten/OTG (Gadget-Modus).
Kein USB-A Port — daher I2S-Mikrofon statt USB-Mikrofon.
eMMC auf dem CM5 ersetzt die microSD-Karte (schneller, keine Extra-Kosten).

### Optional (beide Varianten)

| Teil | Beispiel | ca. Preis |
|------|----------|-----------|
| RGB-LED (gemeinsame Kathode) | Status-Feedback | ~1 EUR |
| 10k Widerstand | Pull-up fuer Taster | ~0.10 EUR |
| Breadboard + Jumperkabel | Fuer Prototyping | ~5 EUR |

### Nicht noetig

| Teil | Warum nicht |
|------|-------------|
| Netzteil | Strom kommt vom Host ueber USB-C |
| HDMI-Kabel/Monitor | Headless — Setup ueber SSH oder Config auf USB-Stick |
| Tastatur/Maus | Headless-Betrieb |

## Architektur-Eingriffe in voicsh

voicsh hat saubere Trait-Abstraktionen. Die Integration erfordert drei neue Komponenten:

### 1. Feature Gate: `usb-hid`

```toml
# Cargo.toml
[features]
usb-hid = []
pi = ["usb-hid", "cpal-audio", "whisper", "cli"]
```

### 2. `UsbHidSink` — neues TextSink Backend

**Datei:** `src/inject/usb_hid.rs`

Implementiert `TextSink` — schreibt HID-Reports direkt nach `/dev/hidg0`.

```
Pipeline:  ... → TranscriberStation → PostProcessorStation → SinkStation(UsbHidSink)
                                                                    │
                                                                    ▼
                                                              /dev/hidg0
                                                                    │
                                                                    ▼
                                                            USB → Host-Rechner
```

```rust
struct HidKeyboardReport {
    modifier: u8,
    reserved: u8,
    keys: [u8; 6],
}

struct UsbHidSink {
    device: std::fs::File,  // /dev/hidg0
    layout: KeyboardLayout, // US, DE
}

impl TextSink for UsbHidSink {
    fn handle(&mut self, text: &str) -> Result<()> {
        for ch in text.chars() {
            let report = self.layout.char_to_report(ch);
            self.device.write_all(&report.to_bytes())?;
            self.device.write_all(&HidKeyboardReport::EMPTY)?; // key release
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }

    fn handle_events(&mut self, events: &[SinkEvent]) -> Result<()> {
        for event in events {
            match event {
                SinkEvent::Text(t) => self.handle(t)?,
                SinkEvent::Backspace => self.send_key(KEY_BACKSPACE, 0)?,
                SinkEvent::Key { ctrl, shift, key } => {
                    let mods = if *ctrl { MOD_CTRL } else { 0 }
                             | if *shift { MOD_SHIFT } else { 0 };
                    self.send_key(self.layout.key_to_scancode(*key), mods)?;
                }
            }
        }
        Ok(())
    }
}
```

Voice Commands funktionieren automatisch — `UsbHidSink` verarbeitet dieselben
`SinkEvent`s wie `InjectorSink`, nur die Ausgabe geht ueber USB statt Wayland.

### 3. GPIO-Button: Push-to-Talk

**Datei:** `src/input/gpio_button.rs`

```
GPIO 17 → 10k Pull-up → 3.3V
         └→ GND beim Druecken
```

Drei Modi:

| Modus | Verhalten |
|-------|-----------|
| `hold` | Aufnehmen solange gedrueckt |
| `toggle` | Einmal = Start, nochmal = Stop |
| `off` | Kontinuierlich (VAD-only) |

Integration ueber bestehende IPC-Commands (`Start`/`Stop`/`Toggle`):

```rust
fn gpio_button_loop(pin: u8, socket_path: &Path, mode: PttMode) {
    match mode {
        PttMode::Hold => loop {
            wait_for_edge(pin, Edge::Falling);
            send_ipc_command(socket_path, Command::Start);
            wait_for_edge(pin, Edge::Rising);
            send_ipc_command(socket_path, Command::Stop);
        },
        PttMode::Toggle => loop {
            wait_for_edge(pin, Edge::Falling);
            send_ipc_command(socket_path, Command::Toggle);
        },
        PttMode::Off => {}
    }
}
```

### 4. LED-Feedback

**Datei:** `src/output/led.rs`

| Status | LED | Trigger |
|--------|-----|---------|
| Idle | Aus / pulsend | Daemon wartet auf Knopf |
| Aufnahme | Gruen | AudioSource aktiv |
| Transkription | Gelb | TranscriberStation arbeitet |
| Fertig | Gruen kurz | SinkStation fertig |
| Fehler | Rot | Pipeline-Fehler |

Subscribed auf `DaemonEvent`-Kanal — wie die GNOME Extension.

## USB Gadget Setup

### Kernel

```bash
# /boot/config.txt
dtoverlay=dwc2

# /etc/modules
dwc2
libcomposite
```

### Gadget-Script

```bash
#!/bin/bash
# /usr/local/bin/voicsh-gadget-setup.sh

GADGET=/sys/kernel/config/usb_gadgets/voicsh
CONFIG_IMG=/var/lib/voicsh/config.img

mkdir -p $GADGET
echo 0x1d6b > $GADGET/idVendor
echo 0x0104 > $GADGET/idProduct
echo 0x0100 > $GADGET/bcdDevice
echo 0x0200 > $GADGET/bcdUSB

mkdir -p $GADGET/strings/0x409
echo "voicsh" > $GADGET/strings/0x409/manufacturer
echo "voicsh Voice Keyboard" > $GADGET/strings/0x409/product

# HID Keyboard
mkdir -p $GADGET/functions/hid.usb0
echo 1 > $GADGET/functions/hid.usb0/protocol
echo 1 > $GADGET/functions/hid.usb0/subclass
echo 8 > $GADGET/functions/hid.usb0/report_length
echo -ne '\x05\x01\x09\x06\xa1\x01\x05\x07\x19\xe0\x29\xe7\x15\x00\x25\x01\x75\x01\x95\x08\x81\x02\x95\x01\x75\x08\x81\x01\x95\x05\x75\x01\x05\x08\x19\x01\x29\x05\x91\x02\x95\x01\x75\x03\x91\x01\x95\x06\x75\x08\x15\x00\x26\xff\x00\x05\x07\x19\x00\x29\xff\x81\x00\xc0' \
  > $GADGET/functions/hid.usb0/report_desc

# Mass Storage (Config)
mkdir -p $GADGET/functions/mass_storage.usb0
echo 1 > $GADGET/functions/mass_storage.usb0/stall
echo 0 > $GADGET/functions/mass_storage.usb0/lun.0/cdrom
echo 0 > $GADGET/functions/mass_storage.usb0/lun.0/ro
echo 0 > $GADGET/functions/mass_storage.usb0/lun.0/nofua
echo $CONFIG_IMG > $GADGET/functions/mass_storage.usb0/lun.0/file

# Activate
mkdir -p $GADGET/configs/c.1/strings/0x409
echo "Keyboard + Config" > $GADGET/configs/c.1/strings/0x409/configuration
echo 250 > $GADGET/configs/c.1/MaxPower
ln -sf $GADGET/functions/hid.usb0 $GADGET/configs/c.1/
ln -sf $GADGET/functions/mass_storage.usb0 $GADGET/configs/c.1/
ls /sys/class/udc > $GADGET/UDC
```

### Systemd

```ini
# /etc/systemd/system/voicsh-gadget.service
[Unit]
Description=voicsh USB Gadget Setup
After=sys-kernel-config.mount
[Service]
Type=oneshot
ExecStart=/usr/local/bin/voicsh-gadget-setup.sh
RemainAfterExit=yes
[Install]
WantedBy=multi-user.target
```

```ini
# /etc/systemd/system/voicsh-pi.service
[Unit]
Description=voicsh Voice Keyboard
After=voicsh-gadget.service sound.target
Requires=voicsh-gadget.service
[Service]
Type=simple
ExecStart=/usr/local/bin/voicsh daemon --backend usb-hid --gpio-button 17
Restart=on-failure
User=voicsh
Group=gpio
[Install]
WantedBy=multi-user.target
```

## Konfiguration

```toml
# config.toml (auf dem USB-Stick)

[stt]
model = "small"
language = "de"

[injection]
backend = "usb_hid"
layout = "de"

[ptt]
gpio = 17
mode = "hold"    # hold | toggle | off
```

## Implementierungsreihenfolge

### Phase 1: USB HID Sink

1. `usb-hid` Feature Gate in Cargo.toml
2. `src/inject/usb_hid.rs` — `UsbHidSink` implementiert `TextSink`
3. HID Keycode Mapping — ASCII → USB HID Scancodes (US, DE Layout)
4. `SinkEvent` Handling — Text, Backspace, Key-Combos ueber HID Reports
5. `--backend usb-hid` CLI Option
6. Tests — Mock `/dev/hidg0`, verify HID Reports byte-genau
7. Gadget Setup Script + systemd Units

### Phase 2: GPIO + Config Reload

8. `src/input/gpio_button.rs` — Push-to-Talk (hold/toggle/off)
9. `src/output/led.rs` — LED-Feedback ueber GPIO
10. USB Mass Storage Config-Image + inotify Reload

### Phase 3: Distribution

11. Cross-Compilation fuer `aarch64-unknown-linux-gnu`
12. Debian-Paket mit systemd-Units und Gadget-Script

## Performance

Siehe Board-Auswahl oben fuer detaillierte Benchmarks pro Board und Modell.

## Referenzen

- [ARCHITECTURE.md](ARCHITECTURE.md) — Pipeline, Stations, Traits
- [ROADMAP.md](ROADMAP.md) — Phasen und geplante Features
