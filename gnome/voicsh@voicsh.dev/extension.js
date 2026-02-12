// voicsh GNOME Shell Extension
// Communicates with voicsh daemon via Unix socket IPC

import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import St from 'gi://St';
import Shell from 'gi://Shell';
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

        // Icon
        this._icon = new St.Icon({
            icon_name: ICON_DISCONNECTED,
            style_class: 'system-status-icon',
        });
        this._indicator.add_child(this._icon);

        // State
        this._recording = false;
        this._connected = false;
        this._modelName = null;
        this._language = null;

        // Settings (needed before menu for shortcut label, and before keybinding)
        this._settings = this.getSettings();

        // Menu items
        const shortcutLabel = this._formatToggleLabel();
        this._toggleItem = new PopupMenu.PopupMenuItem(shortcutLabel);
        this._toggleItem.connect('activate', () => this._sendCommand('toggle'));
        this._indicator.menu.addMenuItem(this._toggleItem);

        this._indicator.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        this._restartItem = new PopupMenu.PopupMenuItem('Restart Daemon');
        this._restartItem.connect('activate', () => {
            GLib.spawn_command_line_async('systemctl --user restart voicsh.service');
        });
        this._indicator.menu.addMenuItem(this._restartItem);

        this._modelItem = new PopupMenu.PopupMenuItem('Model: —');
        this._modelItem.setSensitive(false);
        this._indicator.menu.addMenuItem(this._modelItem);

        this._languageItem = new PopupMenu.PopupMenuItem('Language: —');
        this._languageItem.setSensitive(false);
        this._indicator.menu.addMenuItem(this._languageItem);

        // Add to panel
        Main.panel.addToStatusArea('voicsh', this._indicator);

        // Keybinding
        Main.wm.addKeybinding(
            'toggle-shortcut',
            this._settings,
            0,
            Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW,
            () => this._sendCommand('toggle'),
        );

        // Start polling
        const interval = this._settings.get_int('poll-interval');
        this._pollId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, interval, () => {
            this._pollStatus();
            return GLib.SOURCE_CONTINUE;
        });

        // Initial poll
        this._pollStatus();
    }

    disable() {
        if (this._pollId) {
            GLib.source_remove(this._pollId);
            this._pollId = null;
        }

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

    async _sendCommand(type) {
        try {
            const response = await this._ipcRequest({ type });
            if (type === 'toggle') {
                this._pollStatus();
            }
            return response;
        } catch (e) {
            this._setDisconnected();
            return null;
        }
    }

    async _pollStatus() {
        try {
            const response = await this._ipcRequest({ type: 'status' });
            if (response && response.type === 'status') {
                this._connected = true;
                this._recording = response.recording;
                this._modelName = response.model_name || null;
                this._language = response.language || null;
                this._updateUi();
            } else {
                this._setDisconnected();
            }
        } catch (e) {
            this._setDisconnected();
        }
    }

    _setDisconnected() {
        this._connected = false;
        this._recording = false;
        this._modelName = null;
        this._language = null;
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
        } else if (this._recording) {
            this._icon.icon_name = ICON_RECORDING;
            this._indicator.add_style_class_name('voicsh-recording');
            this._toggleItem.setSensitive(true);
        } else {
            this._icon.icon_name = ICON_IDLE;
            this._toggleItem.setSensitive(true);
        }

        this._modelItem.label.text = `Model: ${this._modelName || '—'}`;
        this._languageItem.label.text = `Language: ${this._language || '—'}`;
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
