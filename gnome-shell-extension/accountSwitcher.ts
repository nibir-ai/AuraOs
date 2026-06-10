import Clutter from 'gi://Clutter';
import St from 'gi://St';
import Gio from 'gi://Gio';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

interface AccountConfig {
    sub: string;
    email: string;
    display_name: string;
    linux_username: string;
}

export class AccountSwitcherButton extends PanelMenu.Button {
    private _accounts: AccountConfig[] = [];
    private _avatarIcon: St.Icon | null = null;
    private _usernameLabel: St.Label | null = null;

    constructor() {
        super(0.0, 'AuraOS Account Switcher', false);

        this._buildUi();
        this._loadAccounts();
    }

    private _buildUi() {
        const box = new St.BoxLayout({ style_class: 'aura-account-switcher-item' });

        this._avatarIcon = new St.Icon({
            icon_name: 'avatar-default-symbolic',
            style_class: 'system-status-icon',
        });
        box.add_child(this._avatarIcon);

        this._usernameLabel = new St.Label({
            text: 'Google User',
            y_align: Clutter.ActorAlign.CENTER,
        });
        box.add_child(this._usernameLabel);

        this.add_child(box);
    }

    private _loadAccounts() {
        this._accounts = [];
        const accountsDir = Gio.File.new_for_path('/var/lib/auraos/accounts');

        try {
            const enumerator = accountsDir.enumerate_children(
                'standard::*',
                Gio.FileQueryInfoFlags.NONE,
                null
            );

            let info;
            while ((info = enumerator.next_file(null))) {
                const child = enumerator.get_child(info);
                if (child.get_path() && child.get_path()!.endsWith('.json')) {
                    const [success, contents] = Gio.File.new_for_path(child.get_path()!).load_contents(null);
                    if (success) {
                        const decoder = new TextDecoder('utf-8');
                        const data: AccountConfig = JSON.parse(decoder.decode(contents));
                        this._accounts.push(data);
                    }
                }
            }
        } catch (e) {
            log(`Failed to read accounts: ${e}`);
        }

        this._updateMenu();
    }

    private _updateMenu() {
        this.menu.removeAll();

        // 1. Get current login username from env
        const currentUsername = GLib.getenv('USER') || 'unknown';
        const activeAccount = this._accounts.find(a => a.linux_username === currentUsername);

        if (activeAccount && this._usernameLabel) {
            this._usernameLabel.set_text(activeAccount.display_name);
            
            // Try loading user's avatar icon if ~/.face exists
            const facePath = `${GLib.get_home_dir()}/.face`;
            if (Gio.File.new_for_path(facePath).query_exists(null) && this._avatarIcon) {
                const iconFile = Gio.File.new_for_path(facePath);
                const gicon = new Gio.FileIcon({ file: iconFile });
                this._avatarIcon.set_gicon(gicon);
            }
        }

        // 2. Add header
        const header = new PopupMenu.PopupMenuItem('Registered Accounts', { reactive: false });
        this.menu.addMenuItem(header);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        // 3. Add account list items
        for (const account of this._accounts) {
            const isActive = account.linux_username === currentUsername;
            const label = isActive 
                ? `● ${account.display_name} (${account.email})` 
                : `   ${account.display_name} (${account.email})`;
            
            const item = new PopupMenu.PopupMenuItem(label);
            
            if (!isActive) {
                item.connect('activate', () => {
                    this._switchToUser(account.linux_username);
                });
            }
            
            this.menu.addMenuItem(item);
        }

        // 4. Add "Add Account" option
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        const addAccountItem = new PopupMenu.PopupMenuItem('Add Google Account...');
        addAccountItem.connect('activate', () => {
            this._launchOobe();
        });
        this.menu.addMenuItem(addAccountItem);
    }

    private _switchToUser(username: string) {
        // Fast user switch via GDM DisplayManager D-Bus
        try {
            const connection = Gio.DBus.system;
            connection.call(
                'org.gnome.DisplayManager',
                '/org/gnome/DisplayManager/LocalDisplayFactory',
                'org.gnome.DisplayManager.LocalDisplayFactory',
                'CreateTransientDisplay',
                null,
                null,
                Gio.DBusCallFlags.NONE,
                -1,
                null,
                (conn: any, res: any) => {
                    try {
                        conn.call_finish(res);
                        log(`Switching display for user ${username}`);
                    } catch (err) {
                        // Fallback: systemctl or dm-tool switch
                        Gio.Subprocess.new(
                            ['dm-tool', 'switch-to-user', username],
                            Gio.SubprocessFlags.NONE
                        );
                    }
                }
            );
        } catch (e) {
            logError(e, 'Failed to trigger fast user switch');
        }
    }

    private _launchOobe() {
        // Trigger aura-auth-helper to add a new account
        try {
            Gio.Subprocess.new(
                ['pkexec', '/usr/lib/auraos/aura-auth-helper'],
                Gio.SubprocessFlags.NONE
            );
        } catch (e) {
            logError(e, 'Failed to launch OOBE assistant');
        }
    }
}
