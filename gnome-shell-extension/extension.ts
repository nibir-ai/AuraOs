import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';
import { GeminiDBusClient } from './dbusClient.js';
import { GeminiPanelButton } from './geminiPanel.js';
import { AccountSwitcherButton } from './accountSwitcher.js';

export default class GeminiAssistantExtension extends Extension {
    private _dbusClient: GeminiDBusClient | null = null;
    private _geminiPanelButton: GeminiPanelButton | null = null;
    private _accountSwitcherButton: AccountSwitcherButton | null = null;

    enable() {
        log('Enabling Gemini Assistant Extension on AuraOS');

        // 1. Initialize D-Bus connection
        this._dbusClient = new GeminiDBusClient();

        // 2. Add Gemini chat panel button to top right
        this._geminiPanelButton = new GeminiPanelButton(this._dbusClient);
        Main.panel.addToStatusArea('gemini-assistant', this._geminiPanelButton, 0, 'right');

        // 3. Add Google accounts switcher to top right
        this._accountSwitcherButton = new AccountSwitcherButton();
        Main.panel.addToStatusArea('aura-account-switcher', this._accountSwitcherButton, 1, 'right');
    }

    disable() {
        log('Disabling Gemini Assistant Extension on AuraOS');

        if (this._geminiPanelButton) {
            this._geminiPanelButton.destroy();
            this._geminiPanelButton = null;
        }

        if (this._accountSwitcherButton) {
            this._accountSwitcherButton.destroy();
            this._accountSwitcherButton = null;
        }

        if (this._dbusClient) {
            this._dbusClient.destroy();
            this._dbusClient = null;
        }
    }
}
