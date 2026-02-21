// voicsh GNOME Shell Extension
// Communicates with voicsh daemon via Unix socket IPC

import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import St from 'gi://St';
import Shell from 'gi://Shell';
import Clutter from 'gi://Clutter';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';

const SOCKET_NAME = 'voicsh.sock';
const ICON_DISCONNECTED = 'microphone-sensitivity-muted-symbolic';
const ICON_IDLE = 'microphone-sensitivity-high-symbolic';
const ICON_RECORDING = 'audio-input-microphone-symbolic';

export default class VoicshExtension extends Extension {
    enable() {
        this._indicator = new PanelMenu.Button(0.0, 'voicsh', false);

        // Icon + language label
        const box = new St.BoxLayout({ style_class: 'panel-status-menu-box' });
        this._icon = new St.Icon({
            icon_name: ICON_DISCONNECTED,
            style_class: 'system-status-icon',
        });
        this._langLabel = new St.Label({
            style_class: 'voicsh-lang-label',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._langLabel.visible = false;
        box.add_child(this._icon);
        box.add_child(this._langLabel);
        this._indicator.add_child(box);

        // State
        this._recording = false;
        this._connected = false;
        this._modelName = null;
        this._language = null;
        this._lastLevel = 0;
        this._lastThreshold = 0.02;
        this._lastIsSpeech = false;
        this._modelLoading = false;
        this._modelLoadProgress = null;
        this._followConnection = null;
        this._followReader = null;
        this._followCancellable = null;
        this._binaryPath = null;
        this._followBackoff = 1;
        this._reconnectId = null;
        this._pulseId = null;
        this._showQuantized = false;
        this._daemonVersion = null;
        this._errorCorrectionEnabled = false;
        this._correctionModel = null;

        // Settings (needed before menu for shortcut label, and before keybinding)
        this._settings = this.getSettings();

        // Menu items
        const shortcutLabel = this._formatToggleLabel();
        this._toggleItem = new PopupMenu.PopupMenuItem(shortcutLabel);
        this._toggleItem.connect('activate', () => this._sendCommand('toggle'));
        this._indicator.menu.addMenuItem(this._toggleItem);

        // Level bar (only visible when recording and menu is open)
        this._levelBarItem = new PopupMenu.PopupBaseMenuItem({ reactive: false });
        this._levelBox = new St.Widget({ style_class: 'voicsh-level-box' });
        this._levelFill = new St.Widget({ style_class: 'voicsh-level-fill' });
        this._thresholdMarker = new St.Widget({ style_class: 'voicsh-threshold-marker' });
        this._levelBox.add_child(this._levelFill);
        this._levelBox.add_child(this._thresholdMarker);
        this._levelBarItem.add_child(this._levelBox);
        this._indicator.menu.addMenuItem(this._levelBarItem);
        this._levelBarItem.visible = false;

        this._indicator.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        this._restartItem = new PopupMenu.PopupMenuItem('Restart Daemon');
        this._restartItem.connect('activate', () => {
            GLib.spawn_command_line_async('systemctl --user restart voicsh.service');
        });
        this._indicator.menu.addMenuItem(this._restartItem);

        this._debugItem = new PopupMenu.PopupMenuItem('Open Debug Log');
        this._debugItem.connect('activate', () => {
            const bin = GLib.shell_quote(this._binaryPath || 'voicsh');
            // Launch terminal with voicsh follow for live debugging
            try {
                GLib.spawn_command_line_async(
                    `gnome-terminal -- bash -c "${bin} follow; read -p 'Press Enter to close...'"`,
                );
            } catch (e) {
                // Try alternative terminals
                try {
                    GLib.spawn_command_line_async(
                        `xterm -e "${bin} follow; read -p 'Press Enter to close...'"`,
                    );
                } catch (e2) {
                    // Fallback: just try to open voicsh follow
                    GLib.spawn_command_line_async(`${bin} follow`);
                }
            }
        });
        this._indicator.menu.addMenuItem(this._debugItem);

        this._languageMenu = new PopupMenu.PopupSubMenuMenuItem('Language: —');
        this._indicator.menu.addMenuItem(this._languageMenu);

        this._modelMenu = new PopupMenu.PopupSubMenuMenuItem('Model: —');
        this._indicator.menu.addMenuItem(this._modelMenu);

        this._correctionMenu = new PopupMenu.PopupSubMenuMenuItem('Correction: off (English)');
        this._indicator.menu.addMenuItem(this._correctionMenu);

        this._indicator.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this._versionItem = new PopupMenu.PopupBaseMenuItem({ reactive: false });
        this._versionLabel = new St.Label({ text: 'voicsh', style_class: 'voicsh-version-label' });
        this._versionItem.add_child(this._versionLabel);
        this._indicator.menu.addMenuItem(this._versionItem);

        // Add to panel
        Main.panel.addToStatusArea('voicsh', this._indicator);

        // Populate menus on open
        this._indicator.menu.connect('open-state-changed', (menu, isOpen) => {
            if (isOpen) {
                this._populateLanguageMenu();
                this._populateModelMenu();
                this._populateCorrectionMenu();
                if (this._recording) {
                    this._updateLevelBar();
                }
            }
        });

        // Keybinding
        Main.wm.addKeybinding(
            'toggle-shortcut',
            this._settings,
            0,
            Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW,
            () => this._sendCommand('toggle'),
        );

        // Start follow mode (persistent connection)
        this._startFollow();
    }

    disable() {
        this._closeFollow();

        if (this._reconnectId) {
            GLib.source_remove(this._reconnectId);
            this._reconnectId = null;
        }

        this._stopPulse();

        Main.wm.removeKeybinding('toggle-shortcut');

        if (this._indicator) {
            this._indicator.destroy();
            this._indicator = null;
        }

        this._settings = null;
    }

    _getSocketPath() {
        const runtimeDir = GLib.get_user_runtime_dir();
        return GLib.build_filenamev([runtimeDir, SOCKET_NAME]);
    }

    async _sendCommand(commandOrType) {
        try {
            const command = typeof commandOrType === 'string'
                ? { type: commandOrType }
                : commandOrType;
            const response = await this._ipcRequest(command);
            return response;
        } catch (e) {
            this._setDisconnected();
            return null;
        }
    }

    _startFollow() {
        if (this._followConnection) return;
        this._followBackoff = 1;
        this._connectFollow();
    }

    _connectFollow() {
        try {
            const socketPath = this._getSocketPath();
            const address = Gio.UnixSocketAddress.new(socketPath);
            const client = new Gio.SocketClient();

            client.connect_async(address, null, (client, result) => {
                try {
                    this._followConnection = client.connect_finish(result);
                    this._followBackoff = 1;

                    // Send follow command
                    const ostream = this._followConnection.get_output_stream();
                    ostream.write_all(new TextEncoder().encode('{"type":"follow"}\n'), null);

                    // Set up line reader
                    this._followReader = new Gio.DataInputStream({
                        base_stream: this._followConnection.get_input_stream(),
                    });
                    this._followCancellable = new Gio.Cancellable();

                    this._connected = true;
                    this._updateUi();

                    this._readFollowLine();
                } catch (e) {
                    this._scheduleReconnect();
                }
            });
        } catch (e) {
            this._scheduleReconnect();
        }
    }

    _readFollowLine() {
        if (!this._followReader) return;

        this._followReader.read_line_async(GLib.PRIORITY_DEFAULT, this._followCancellable, (stream, res) => {
            try {
                const [line] = stream.read_line_finish_utf8(res);
                if (line) {
                    this._handleFollowEvent(JSON.parse(line));
                    this._readFollowLine();
                } else {
                    // EOF - daemon closed connection
                    this._closeFollow();
                    this._scheduleReconnect();
                }
            } catch (e) {
                this._closeFollow();
                this._scheduleReconnect();
            }
        });
    }

    _handleFollowEvent(event) {
        switch (event.type) {
            case 'recording_state_changed':
                this._recording = event.recording;
                this._updateUi();
                break;
            case 'level':
                this._lastLevel = event.level;
                this._lastThreshold = event.threshold;
                this._lastIsSpeech = event.is_speech;
                if (this._indicator?.menu?.isOpen) {
                    this._updateLevelBar();
                }
                break;
            case 'transcription':
                // Store last transcription for potential debug display
                break;
            case 'transcription_dropped':
                // Filtered transcription — visible in follow/debug log
                break;
            case 'config_changed':
                if (event.key === 'language' && event.value) this._language = event.value;
                if (event.key === 'model' && event.value) this._modelName = event.value;
                if (event.key === 'error_correction') {
                    this._errorCorrectionEnabled = event.value === 'true';
                    this._updateCorrectionMenuLabel();
                }
                if (event.key === 'correction_model') {
                    this._correctionModel = event.value;
                    this._updateCorrectionMenuLabel();
                }
                this._updateUi();
                break;
            case 'model_loading':
                this._modelLoading = true;
                this._modelLoadProgress = event.progress;
                this._updateModelMenu();
                break;
            case 'model_loaded':
                this._modelLoading = false;
                this._modelName = event.model;
                this._updateModelMenu();
                this._updateUi();
                break;
            case 'model_load_failed':
                this._modelLoading = false;
                this._updateModelMenu();
                break;
            case 'daemon_info':
                this._binaryPath = event.binary_path || null;
                this._daemonVersion = event.version || null;
                this._updateVersionLabel();
                break;
            case 'log':
                // ignore
                break;
        }
    }

    _closeFollow() {
        if (this._reconnectId) {
            GLib.source_remove(this._reconnectId);
            this._reconnectId = null;
        }
        if (this._followCancellable) {
            this._followCancellable.cancel();
            this._followCancellable = null;
        }
        if (this._followConnection) {
            try {
                this._followConnection.close(null);
            } catch (e) {
                // ignore
            }
            this._followConnection = null;
        }
        this._followReader = null;
    }

    _scheduleReconnect() {
        this._setDisconnected();
        if (this._reconnectId) return;

        this._reconnectId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, this._followBackoff, () => {
            this._reconnectId = null;
            this._followBackoff = Math.min(this._followBackoff * 2, 10);
            this._connectFollow();
            return GLib.SOURCE_REMOVE;
        });
    }

    _setDisconnected() {
        this._connected = false;
        this._recording = false;
        this._modelName = null;
        this._language = null;
        this._binaryPath = null;
        this._daemonVersion = null;
        this._errorCorrectionEnabled = false;
        this._correctionModel = null;
        this._updateUi();
    }

    _updateUi() {
        if (!this._indicator) return;

        // Remove old style classes
        this._indicator.remove_style_class_name('voicsh-recording');
        this._indicator.remove_style_class_name('voicsh-disconnected');

        if (!this._connected) {
            this._icon.icon_name = ICON_DISCONNECTED;
            this._indicator.add_style_class_name('voicsh-disconnected');
            this._toggleItem.setSensitive(false);
            this._stopPulse();
        } else if (this._recording) {
            this._icon.icon_name = ICON_RECORDING;
            this._indicator.add_style_class_name('voicsh-recording');
            this._toggleItem.setSensitive(true);
            this._toggleItem.label.text = 'Stop Recording';
            this._levelBarItem.visible = true;
            this._startPulse();
        } else {
            this._icon.icon_name = ICON_IDLE;
            this._toggleItem.setSensitive(true);
            const shortcutLabel = this._formatToggleLabel();
            this._toggleItem.label.text = shortcutLabel;
            this._levelBarItem.visible = false;
            this._stopPulse();
        }

        // Language indicator in panel
        if (this._langLabel) {
            const showLabel = this._settings?.get_boolean('show-language-label') ?? true;
            if (showLabel && this._language && this._language !== 'auto') {
                this._langLabel.text = this._language.substring(0, 2).toUpperCase();
                this._langLabel.visible = true;
            } else {
                this._langLabel.visible = false;
            }
        }

        // Update menu labels
        const langLabel = this._language === 'auto' ? 'Auto' : (this._language || '—').toUpperCase();
        this._languageMenu.label.text = `Language: ${langLabel}`;
        this._updateModelMenu();
        this._updateCorrectionMenuLabel();
        this._updateVersionLabel();
    }

    _updateModelMenu() {
        if (!this._modelMenu) return;

        if (this._modelLoading) {
            const progress = this._modelLoadProgress ? ` (${this._modelLoadProgress})` : '';
            this._modelMenu.label.text = `Model: loading...${progress}`;
        } else {
            this._modelMenu.label.text = `Model: ${this._modelName || '—'}`;
        }
    }

    _updateLevelBar() {
        if (!this._levelFill || !this._indicator?.menu?.isOpen || !this._recording) return;

        const level = Math.min(this._lastLevel || 0, 1.0);
        const threshold = Math.min(this._lastThreshold || 0.02, 1.0);

        // Scale level to percentage (amplify for visibility)
        const pct = Math.min(level * 500, 100);
        this._levelFill.set_size(Math.round(pct * 2), 8);

        // Color based on speech detection
        if (this._lastIsSpeech) {
            this._levelFill.add_style_class_name('voicsh-level-speech');
            this._levelFill.remove_style_class_name('voicsh-level-idle');
        } else {
            this._levelFill.add_style_class_name('voicsh-level-idle');
            this._levelFill.remove_style_class_name('voicsh-level-speech');
        }

        // Threshold marker position
        const thresholdPct = Math.min(threshold * 500, 100);
        this._thresholdMarker.set_position(Math.round(thresholdPct * 2), 0);
        this._thresholdMarker.set_size(2, 12);
    }

    _startPulse() {
        if (this._pulseId) return;
        this._pulseId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 750, () => {
            if (!this._icon) {
                this._pulseId = null;
                return GLib.SOURCE_REMOVE;
            }
            this._icon.ease({
                opacity: this._icon.opacity < 200 ? 255 : 100,
                duration: 750,
                mode: Clutter.AnimationMode.EASE_IN_OUT_SINE,
            });
            return GLib.SOURCE_CONTINUE;
        });
    }

    _stopPulse() {
        if (this._pulseId) {
            GLib.source_remove(this._pulseId);
            this._pulseId = null;
        }
        if (this._icon) this._icon.opacity = 255;
    }

    async _populateLanguageMenu() {
        try {
            const response = await this._ipcRequest({ type: 'list_languages' });
            if (response?.type !== 'languages') return;

            this._languageMenu.menu.removeAll();

            for (const lang of response.languages) {
                const label = lang === 'auto' ? 'Auto-detect' : lang.toUpperCase();
                const item = new PopupMenu.PopupMenuItem(label);
                if (lang === response.current) {
                    item.setOrnament(PopupMenu.Ornament.CHECK);
                }
                item.connect('activate', () => {
                    this._sendCommand({ type: 'set_language', language: lang });
                });
                this._languageMenu.menu.addMenuItem(item);
            }

            const currentLabel = response.current === 'auto' ? 'Auto' : response.current.toUpperCase();
            this._languageMenu.label.text = `Language: ${currentLabel}`;
        } catch (e) {
            // Keep existing label
        }
    }

    async _populateModelMenu() {
        try {
            const response = await this._ipcRequest({ type: 'list_models' });
            if (response?.type !== 'models') return;

            this._modelMenu.menu.removeAll();

            for (const model of response.models) {
                if (model.quantized && !this._showQuantized) continue;

                const suffix = model.installed ? ` (${model.size_mb} MB)` : ` (${model.size_mb} MB, download)`;
                const quantTag = model.quantized ? ' [Q]' : '';
                const label = `${model.name}${quantTag}${suffix}`;
                const item = new PopupMenu.PopupMenuItem(label);
                if (model.name === response.current) {
                    item.setOrnament(PopupMenu.Ornament.CHECK);
                }
                item.connect('activate', () => {
                    this._sendCommand({ type: 'set_model', model: model.name });
                });
                this._modelMenu.menu.addMenuItem(item);
            }

            this._modelMenu.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
            const toggleLabel = this._showQuantized ? 'Hide quantized' : 'Show quantized';
            const toggleItem = new PopupMenu.PopupMenuItem(toggleLabel);
            toggleItem.connect('activate', () => {
                this._showQuantized = !this._showQuantized;
                this._populateModelMenu();
            });
            this._modelMenu.menu.addMenuItem(toggleItem);

            this._modelMenu.label.text = `Model: ${response.current}`;
        } catch (e) {
            // Keep existing label
        }
    }

    async _populateCorrectionMenu() {
        try {
            const response = await this._ipcRequest({ type: 'list_correction_models' });
            if (response?.type !== 'correction_models') return;

            this._correctionMenu.menu.removeAll();
            this._errorCorrectionEnabled = response.enabled;
            this._correctionModel = response.current;

            // Toggle on/off
            const toggleLabel = response.enabled ? 'Disable Error Correction' : 'Enable Error Correction';
            const toggleItem = new PopupMenu.PopupMenuItem(toggleLabel);
            toggleItem.connect('activate', () => {
                this._sendCommand({ type: 'set_error_correction', enabled: !response.enabled });
            });
            this._correctionMenu.menu.addMenuItem(toggleItem);

            this._correctionMenu.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

            // Model picker
            for (const model of response.models) {
                const item = new PopupMenu.PopupMenuItem(`${model.display_name}`);
                if (model.name === response.current) {
                    item.setOrnament(PopupMenu.Ornament.CHECK);
                }
                if (!response.enabled) {
                    item.setSensitive(false);
                }
                item.connect('activate', () => {
                    this._sendCommand({ type: 'set_correction_model', model: model.name });
                });
                this._correctionMenu.menu.addMenuItem(item);
            }

            // Update parent label
            this._updateCorrectionMenuLabel();

            // Grey out if not English
            const isEnglish = !this._language || this._language === 'en' || this._language === 'auto';
            this._correctionMenu.setSensitive(isEnglish);
        } catch (e) {
            console.debug(`voicsh: failed to populate correction menu: ${e.message}`);
        }
    }

    _updateCorrectionMenuLabel() {
        if (!this._correctionMenu) return;
        if (this._errorCorrectionEnabled) {
            const model = this._correctionModel || 'flan-t5-small';
            this._correctionMenu.label.text = `Correction: ${model} (English)`;
        } else {
            this._correctionMenu.label.text = 'Correction: off (English)';
        }
    }

    _updateVersionLabel() {
        if (!this._versionLabel) return;
        if (this._connected && this._daemonVersion) {
            this._versionLabel.text = `voicsh v${this._daemonVersion}`;
        } else {
            this._versionLabel.text = 'voicsh';
        }
    }

    _formatToggleLabel() {
        const shortcut = this._settings?.get_strv('toggle-shortcut')[0] || '';
        if (!shortcut) return 'Toggle Recording';
        const formatted = this._formatShortcut(shortcut);
        return formatted ? `Toggle Recording  (${formatted})` : 'Toggle Recording';
    }

    _formatShortcut(accel) {
        // Parse GTK accelerator format like '<Super><Alt>v' into 'Super+Alt+V'
        const parts = [];
        let remaining = accel;
        const modifiers = [
            ['<Super>', 'Super'],
            ['<Meta>', 'Super'],
            ['<Ctrl>', 'Ctrl'],
            ['<Control>', 'Ctrl'],
            ['<Alt>', 'Alt'],
            ['<Shift>', 'Shift'],
        ];
        for (const [token, label] of modifiers) {
            if (remaining.includes(token)) {
                parts.push(label);
                remaining = remaining.replace(token, '');
            }
        }
        if (remaining) parts.push(remaining.toUpperCase());
        return parts.join('+');
    }

    _ipcRequest(command) {
        return new Promise((resolve, reject) => {
            try {
                const socketPath = this._getSocketPath();
                const address = Gio.UnixSocketAddress.new(socketPath);
                const client = new Gio.SocketClient();

                client.connect_async(address, null, (client, result) => {
                    try {
                        const connection = client.connect_finish(result);
                        const message = JSON.stringify(command) + '\n';
                        const ostream = connection.get_output_stream();
                        ostream.write_all(new TextEncoder().encode(message), null);

                        const istream = new Gio.DataInputStream({
                            base_stream: connection.get_input_stream(),
                        });

                        istream.read_line_async(GLib.PRIORITY_DEFAULT, null, (stream, res) => {
                            try {
                                const [line] = stream.read_line_finish_utf8(res);
                                connection.close(null);
                                if (line) {
                                    resolve(JSON.parse(line));
                                } else {
                                    reject(new Error('Empty response'));
                                }
                            } catch (e) {
                                reject(e);
                            }
                        });
                    } catch (e) {
                        reject(e);
                    }
                });
            } catch (e) {
                reject(e);
            }
        });
    }
}
