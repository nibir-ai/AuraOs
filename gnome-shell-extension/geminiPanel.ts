import Clutter from 'gi://Clutter';
import St from 'gi://St';
import Gio from 'gi://Gio';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import { GeminiDBusClient } from './dbusClient.js';

export class GeminiPanelButton extends PanelMenu.Button {
    private _dbusClient: GeminiDBusClient;
    private _chatContainer: St.BoxLayout | null = null;
    private _historyBox: St.BoxLayout | null = null;
    private _scrollView: St.ScrollView | null = null;
    private _textEntry: St.Entry | null = null;
    private _sendBtn: St.Button | null = null;
    private _currentAssistantBubble: St.Label | null = null;
    private _currentAssistantText: string = '';

    constructor(dbusClient: GeminiDBusClient) {
        super(0.0, 'Gemini Assistant', false);
        this._dbusClient = dbusClient;

        this._buildPanelButton();
        this._buildChatDropdown();
        this._setupDBusListeners();
    }

    private _buildPanelButton() {
        const box = new St.BoxLayout({ style_class: 'gemini-panel-button' });
        
        const icon = new St.Icon({
            icon_name: 'face-cool-symbolic', // Sparkly assistant feel icon
            style_class: 'gemini-panel-icon',
        });
        box.add_child(icon);

        const label = new St.Label({
            text: 'Gemini',
            y_align: Clutter.ActorAlign.CENTER,
        });
        box.add_child(label);

        this.add_child(box);
    }

    private _buildChatDropdown() {
        // Main container inside the popup menu
        this._chatContainer = new St.BoxLayout({
            vertical: true,
            style_class: 'gemini-chat-panel',
        });

        // 1. Header
        const header = new St.BoxLayout({ style_class: 'gemini-chat-header' });
        const title = new St.Label({
            text: 'Gemini Assistant',
            style_class: 'gemini-chat-title',
            x_expand: true,
        });
        header.add_child(title);

        const clearBtn = new St.Button({
            label: 'Clear',
            style_class: 'gemini-clear-button',
        });
        clearBtn.connect('clicked', () => this._clearChat());
        header.add_child(clearBtn);

        this._chatContainer.add_child(header);

        // 2. Scrollable History Area
        this._scrollView = new St.ScrollView({
            style_class: 'gemini-chat-scroll',
            x_fill: true,
            y_fill: true,
            y_align: St.Align.FILL,
            x_expand: true,
            y_expand: true,
        });
        this._scrollView.set_policy(St.PolicyType.NEVER, St.PolicyType.AUTOMATIC);

        this._historyBox = new St.BoxLayout({
            vertical: true,
            style_class: 'gemini-chat-history',
            x_expand: true,
        });
        this._scrollView.add_actor(this._historyBox);
        this._chatContainer.add_child(this._scrollView);

        // 3. Input Row
        const inputRow = new St.BoxLayout({ style_class: 'gemini-input-container' });
        
        this._textEntry = new St.Entry({
            hint_text: 'Ask Gemini anything...',
            style_class: 'gemini-chat-entry',
            x_expand: true,
            can_focus: true,
        });
        
        // Handle Enter keypress
        const entryClutterText = this._textEntry.clutter_text;
        entryClutterText.connect('key-press-event', (actor: any, event: any) => {
            const symbol = event.get_key_symbol();
            if (symbol === Clutter.KEY_Return || symbol === Clutter.KEY_KP_Enter) {
                this._sendMessage();
                return Clutter.EVENT_STOP;
            }
            return Clutter.EVENT_PROPAGATE;
        });
        
        inputRow.add_child(this._textEntry);

        this._sendBtn = new St.Button({
            label: 'Send',
            style_class: 'gemini-send-button',
        });
        this._sendBtn.connect('clicked', () => this._sendMessage());
        inputRow.add_child(this._sendBtn);

        this._chatContainer.add_child(inputRow);

        // Add container to the menu actor
        this.menu.box.add_child(this._chatContainer);

        // Load history when menu is shown
        this.menu.connect('open-state-changed', (menu: any, open: boolean) => {
            if (open) {
                this._loadHistory();
                this._textEntry?.grab_key_focus();
            }
        });
    }

    private _setupDBusListeners() {
        // Handle streaming chunks
        this._dbusClient.on('StreamChunk', (data: { taskId: string; chunk: string; isFinal: boolean }) => {
            if (this._currentAssistantBubble) {
                this._currentAssistantText += data.chunk;
                this._currentAssistantBubble.set_text(this._currentAssistantText);
                this._scrollToBottom();
            }
        });

        // Handle task completions
        this._dbusClient.on('TaskCompleted', (data: { taskId: string; result: any; toolsUsed: string[] }) => {
            this._currentAssistantBubble = null;
            this._currentAssistantText = '';
        });
    }

    private async _loadHistory() {
        if (!this._historyBox) return;
        this._historyBox.destroy_all_children();

        try {
            const history = await this._dbusClient.getConversationHistory();
            for (const turn of history) {
                const role = turn.role; // "user", "model", or "tool"
                const content = turn.content || '';
                
                if (role === 'user') {
                    this._addMessageBubble(content, 'user');
                } else if (role === 'model' && content !== '') {
                    this._addMessageBubble(content, 'assistant');
                }
            }
            this._scrollToBottom();
        } catch (e) {
            logError(e, 'Failed to load conversation history');
        }
    }

    private _addMessageBubble(text: string, sender: 'user' | 'assistant'): St.Label {
        const styleClass = sender === 'user' ? 'gemini-message-user' : 'gemini-message-assistant';
        
        const bubbleBox = new St.BoxLayout({
            x_expand: true,
            pack_start: sender === 'assistant',
        });

        const bubble = new St.Label({
            text: text,
            style_class: `gemini-message ${styleClass}`,
            line_wrap: true,
        });

        bubbleBox.add_child(bubble);
        this._historyBox?.add_child(bubbleBox);
        this._scrollToBottom();
        
        return bubble;
    }

    private async _sendMessage() {
        if (!this._textEntry || !this._dbusClient) return;

        const prompt = this._textEntry.get_text().trim();
        if (prompt === '') return;

        // 1. Add user bubble in UI
        this._addMessageBubble(prompt, 'user');
        this._textEntry.set_text('');

        // 2. Prepare assistant bubble for streaming
        this._currentAssistantText = '';
        this._currentAssistantBubble = this._addMessageBubble('Thinking...', 'assistant');

        // 3. Dispatch the task to the daemon
        try {
            // Context JSON can contain window info, focused app etc. (empty for now)
            await this._dbusClient.dispatchTask(prompt, '{}');
        } catch (e) {
            if (this._currentAssistantBubble) {
                this._currentAssistantBubble.set_text('Error: Could not connect to Gemini system service.');
            }
            logError(e, 'Failed to dispatch task via D-Bus client');
        }
    }

    private async _clearChat() {
        try {
            await this._dbusClient.clearConversation();
            this._historyBox?.destroy_all_children();
            this._currentAssistantBubble = null;
            this._currentAssistantText = '';
        } catch (e) {
            logError(e, 'Failed to clear chat');
        }
    }

    private _scrollToBottom() {
        GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
            if (this._scrollView) {
                const adj = this._scrollView.get_vscroll_bar().get_adjustment();
                adj.set_value(adj.get_upper() - adj.get_page_size());
            }
            return GLib.SOURCE_REMOVE;
        });
    }

    destroy() {
        if (this._chatContainer) {
            this._chatContainer.destroy();
        }
        super.destroy();
    }
}
