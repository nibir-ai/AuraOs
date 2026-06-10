// gnome-shell.d.ts — GJS and GNOME Shell TypeScript Module Declarations
//
// Declares typing interfaces for GNOME Shell internal modules and the
// GJS GObject-Introspection (gi://) namespaces to fix TypeScript compiler resolution.

declare function log(message: string): void;

declare module 'gi://Clutter' {
    const Clutter: any;
    export default Clutter;
}

declare module 'gi://St' {
    const St: any;
    export default St;
}

declare module 'gi://Gio' {
    const Gio: any;
    export default Gio;
}

declare module 'gi://GLib' {
    const GLib: any;
    export default GLib;
}

declare module 'resource:///org/gnome/shell/ui/main.js' {
    export const panel: any;
}

declare module 'resource:///org/gnome/shell/extensions/extension.js' {
    export class Extension {
        static lookupByUUID(uuid: string): Extension;
        enable(): void;
        disable(): void;
    }
}

declare module 'resource:///org/gnome/shell/ui/panelMenu.js' {
    export class Button {
        constructor(menuAlignment: number, name: string, dontCreateMenu?: boolean);
        menu: any;
        add_child(actor: any): void;
        destroy(): void;
    }
}

declare module 'resource:///org/gnome/shell/ui/popupMenu.js' {
    export class PopupBaseMenuItem {
        constructor(params?: any);
        add_child(actor: any): void;
    }
    export class PopupSubMenuMenuItem extends PopupBaseMenuItem {
        constructor(text: string, wantIcon?: boolean);
        menu: any;
    }
    export class PopupSeparatorMenuItem extends PopupBaseMenuItem {
        constructor();
    }
}
