// main.rs — Custom GTK4/Libadwaita Installer Application for AuraOS
//
// Guides the user through a welcome, disk formatting selection, Google account login,
// installation progression, and system reboot.
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use adw::prelude::*;
use adw::{Application, ApplicationWindow, HeaderBar, ViewStack};
use gtk::{
    Align, Button, Entry, Label, ListBox, ListBoxRow, Orientation, ProgressBar, ScrolledWindow,
    SelectionMode,
};
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;
use std::sync::Arc;
use std::thread;

mod install;
use install::{run_install, InstallConfig};

#[derive(Clone)]
struct DiskInfo {
    name: String,  // e.g. "sda"
    model: String, // e.g. "Samsung SSD"
    size: String,  // e.g. "500G"
}

fn main() -> glib::ExitCode {
    // Initialize Libadwaita
    adw::init().expect("Failed to initialize Libadwaita");

    let app = Application::new(Some("org.auraos.installer"), Default::default());
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &Application) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("AuraOS Installation Wizard")
        .default_width(720)
        .default_height(540)
        .resizable(false)
        .build();

    // ─── Initialize Premium CSS Engine ───
    let display = gtk::gdk::Display::default().expect("Could not get default display");
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        r#"
        /* System window background wallpaper */
        window {
            background-image: url('file:///usr/share/backgrounds/auraos/wallpaper.jpg');
            background-size: cover;
            background-position: center;
        }

        /* Centered Glassmorphism Card Container */
        .glass-card {
            background-color: rgba(26, 27, 30, 0.82);
            border: 1px solid rgba(255, 255, 255, 0.08);
            border-radius: 16px;
            padding: 30px;
            margin: 20px;
            box-shadow: 0 10px 40px 0 rgba(0, 0, 0, 0.45);
        }

        /* Titles and Typography */
        .gradient-title {
            font-size: 26pt;
            font-weight: 800;
            color: #ffffff;
            margin-bottom: 4px;
        }
        
        .sub-desc {
            font-size: 11pt;
            color: #b2bec3;
            line-height: 1.4;
        }

        /* Custom buttons styling */
        .pill-button {
            border-radius: 20px;
            padding: 10px 24px;
            font-weight: bold;
        }

        /* Storage devices listbox */
        list {
            background-color: rgba(45, 52, 54, 0.35);
            border-radius: 10px;
            border: 1px solid rgba(255, 255, 255, 0.05);
        }
        
        row {
            border-radius: 8px;
            margin: 2px;
            color: #ffffff;
        }
        
        row:selected {
            background-color: #3867d6;
            color: #ffffff;
        }
        "#
    );
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let view_stack = ViewStack::new();
    view_stack.set_transition_type(adw::ViewStackTransitionType::SlideLeftRight);

    // Selected disk state
    let selected_disk: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    // ─── Page 1: Welcome Page ───
    let welcome_outer = gtk::Box::new(Orientation::Vertical, 0);
    welcome_outer.set_valign(Align::Center);
    welcome_outer.set_halign(Align::Center);

    let welcome_card = gtk::BoxLayout::builder()
        .orientation(Orientation::Vertical)
        .spacing(20)
        .css_classes(vec!["glass-card"])
        .width_request(520)
        .build();

    let welcome_title = Label::builder()
        .label("Welcome to AuraOS")
        .css_classes(vec!["gradient-title"])
        .build();
    welcome_card.append(&welcome_title);

    let welcome_desc = Label::builder()
        .label("This setup wizard will guide you through installing AuraOS on your hard drive. An active internet connection and Google account are required to configure your secure identity.")
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(vec!["sub-desc"])
        .build();
    welcome_card.append(&welcome_desc);

    let start_btn = Button::builder()
        .label("Get Started")
        .css_classes(vec!["suggested-action", "pill-button"])
        .halign(Align::Center)
        .margin_top(20)
        .build();
    welcome_card.append(&start_btn);
    welcome_outer.append(&welcome_card);

    view_stack.add_titled(&welcome_outer, Some("welcome"), "Welcome");

    // ─── Page 2: Disk Selection Page ───
    let disk_outer = gtk::Box::new(Orientation::Vertical, 0);
    disk_outer.set_valign(Align::Center);
    disk_outer.set_halign(Align::Center);

    let disk_card = gtk::BoxLayout::builder()
        .orientation(Orientation::Vertical)
        .spacing(16)
        .css_classes(vec!["glass-card"])
        .width_request(540)
        .build();

    let disk_title = Label::builder()
        .label("Select System Disk")
        .css_classes(vec!["gradient-title"])
        .build();
    disk_card.append(&disk_title);

    let disk_warning = Label::builder()
        .label("Wiping disk: Formatting deletes all data. Backup critical files first.")
        .wrap(true)
        .css_classes(vec!["error", "sub-desc"])
        .build();
    disk_card.append(&disk_warning);

    let list_box = ListBox::new();
    list_box.set_selection_mode(SelectionMode::Single);

    let disks = get_available_disks();
    for disk in &disks {
        let row = ListBoxRow::new();
        let row_box = gtk::BoxLayout::new(Orientation::Horizontal, 12);
        row_box.set_margin_top(8);
        row_box.set_margin_bottom(8);
        row_box.set_margin_start(10);
        row_box.set_margin_end(10);

        let icon = gtk::Image::from_icon_name("drive-harddisk");
        row_box.append(&icon);

        let details_box = gtk::BoxLayout::new(Orientation::Vertical, 1);
        let name_lbl = Label::builder()
            .label(&format!("{} ({})", disk.model, disk.size))
            .halign(Align::Start)
            .css_classes(vec!["bold"])
            .build();
        let path_lbl = Label::builder()
            .label(&format!("/dev/{}", disk.name))
            .halign(Align::Start)
            .css_classes(vec!["caption"])
            .build();
        details_box.append(&name_lbl);
        details_box.append(&path_lbl);

        row_box.append(&details_box);
        row.set_child(Some(&row_box));
        list_box.append(&row);
    }

    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&list_box)
        .min_content_height(140)
        .build();
    disk_card.append(&scroll);

    let disk_next_btn = Button::builder()
        .label("Confirm Disk")
        .css_classes(vec!["suggested-action", "pill-button"])
        .halign(Align::Center)
        .sensitive(false)
        .build();
    disk_card.append(&disk_next_btn);
    disk_outer.append(&disk_card);

    view_stack.add_titled(&disk_outer, Some("disk"), "Select Disk");

    // Connect Selection listeners
    let selected_disk_clone = selected_disk.clone();
    let disk_next_btn_clone = disk_next_btn.clone();
    let disks_clone = disks.clone();
    list_box.connect_row_selected(move |_, row| {
        if let Some(r) = row {
            let index = r.index() as usize;
            if index < disks_clone.len() {
                let dev_path = format!("/dev/{}", disks_clone[index].name);
                *selected_disk_clone.borrow_mut() = Some(dev_path);
                disk_next_btn_clone.set_sensitive(true);
            }
        }
    });

    // ─── Page 3: Google Account Sign-In Page ───
    let login_outer = gtk::Box::new(Orientation::Vertical, 0);
    login_outer.set_valign(Align::Center);
    login_outer.set_halign(Align::Center);

    let login_card = gtk::BoxLayout::builder()
        .orientation(Orientation::Vertical)
        .spacing(20)
        .css_classes(vec!["glass-card"])
        .width_request(520)
        .build();

    let login_title = Label::builder()
        .label("Sign in with Google")
        .css_classes(vec!["gradient-title"])
        .build();
    login_card.append(&login_title);

    let login_desc = Label::builder()
        .label("AuraOS maps Linux identities directly to Google Accounts. Registering your account credentials now allows local file extraction and configures system preferences before the first boot.")
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(vec!["sub-desc"])
        .build();
    login_card.append(&login_desc);

    let login_btn = Button::builder()
        .label("Log In")
        .css_classes(vec!["suggested-action", "pill-button"])
        .halign(Align::Center)
        .margin_top(10)
        .build();
    login_card.append(&login_btn);

    let login_next_btn = Button::builder()
        .label("Start Copying Files")
        .css_classes(vec!["pill-button"])
        .halign(Align::Center)
        .margin_top(10)
        .sensitive(false)
        .build();
    login_card.append(&login_next_btn);
    login_outer.append(&login_card);

    view_stack.add_titled(&login_outer, Some("login"), "Log In");

    // Trigger auth-helper logic
    let login_next_btn_clone = login_next_btn.clone();
    login_btn.connect_clicked(move |_| {
        let status = Command::new("pkexec")
            .arg("/usr/lib/auraos/aura-auth-helper")
            .status();

        match status {
            Ok(s) if s.success() => {
                login_next_btn_clone.set_sensitive(true);
                login_next_btn_clone.set_css_classes(&["suggested-action", "pill-button"]);
            }
            _ => {
                eprintln!("OAuth login window closed or failed");
            }
        }
    });

    // ─── Page 4: Progress Page ───
    let progress_outer = gtk::Box::new(Orientation::Vertical, 0);
    progress_outer.set_valign(Align::Center);
    progress_outer.set_halign(Align::Center);

    let progress_card = gtk::BoxLayout::builder()
        .orientation(Orientation::Vertical)
        .spacing(24)
        .css_classes(vec!["glass-card"])
        .width_request(520)
        .build();

    let progress_title = Label::builder()
        .label("Installing AuraOS")
        .css_classes(vec!["gradient-title"])
        .build();
    progress_card.append(&progress_title);

    let progress_bar = ProgressBar::new();
    progress_bar.set_show_text(true);
    progress_card.append(&progress_bar);

    let progress_status = Label::builder()
        .label("Analyzing storage block clusters...")
        .wrap(true)
        .css_classes(vec!["sub-desc"])
        .build();
    progress_card.append(&progress_status);
    progress_outer.append(&progress_card);

    view_stack.add_titled(&progress_outer, Some("progress"), "Progress");

    // ─── Page 5: Finished Page ───
    let finished_outer = gtk::Box::new(Orientation::Vertical, 0);
    finished_outer.set_valign(Align::Center);
    finished_outer.set_halign(Align::Center);

    let finished_card = gtk::BoxLayout::builder()
        .orientation(Orientation::Vertical)
        .spacing(20)
        .css_classes(vec!["glass-card"])
        .width_request(520)
        .build();

    let finished_icon = gtk::Image::from_icon_name("object-select-symbolic");
    finished_icon.set_pixel_size(64);
    finished_card.append(&finished_icon);

    let finished_title = Label::builder()
        .label("Installation Successful")
        .css_classes(vec!["gradient-title"])
        .build();
    finished_card.append(&finished_title);

    let finished_desc = Label::builder()
        .label("Your AuraOS installation is ready. You can restart your PC and log in securely using your credentials.")
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(vec!["sub-desc"])
        .build();
    finished_card.append(&finished_desc);

    let reboot_btn = Button::builder()
        .label("Reboot Now")
        .css_classes(vec!["suggested-action", "pill-button"])
        .halign(Align::Center)
        .margin_top(10)
        .build();
    finished_card.append(&reboot_btn);
    finished_outer.append(&finished_card);

    view_stack.add_titled(&finished_outer, Some("finished"), "Finished");

    // Navigation connections
    let view_stack_clone = view_stack.clone();
    start_btn.connect_clicked(move |_| {
        view_stack_clone.set_visible_child_name("disk");
    });

    let view_stack_clone = view_stack.clone();
    disk_next_btn.connect_clicked(move |_| {
        view_stack_clone.set_visible_child_name("login");
    });

    // Start background file system write
    let view_stack_clone = view_stack.clone();
    let selected_disk_clone = selected_disk.clone();
    let progress_bar_clone = progress_bar.clone();
    let progress_status_clone = progress_status.clone();
    login_next_btn.connect_clicked(move |_| {
        view_stack_clone.set_visible_child_name("progress");

        let disk_path = selected_disk_clone.borrow().clone().unwrap_or_default();
        let config = InstallConfig { disk: disk_path };

        let (tx, rx) = glib::MainContext::channel::<(f64, String)>(glib::Priority::default());

        let p_bar = progress_bar_clone.clone();
        let p_status = progress_status_clone.clone();
        let vs = view_stack_clone.clone();

        rx.attach(None, move |(val, status)| {
            p_bar.set_fraction(val);
            p_status.set_label(&status);

            if val >= 1.0 {
                vs.set_visible_child_name("finished");
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });

        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            let tx_clone = tx.clone();
            let install_res = rt.block_on(async {
                run_install(config, move |frac, status| {
                    let _ = tx_clone.send((frac, status.to_string()));
                })
                .await
            });

            match install_res {
                Ok(_) => {
                    let _ = tx.send((1.0, "Done".to_string()));
                }
                Err(e) => {
                    let _ = tx.send((0.0, format!("Error: {}", e)));
                }
            }
        });
    });

    reboot_btn.connect_clicked(|_| {
        let _ = Command::new("systemctl").arg("reboot").status();
    });

    // Layout assembly
    let header_bar = HeaderBar::new();
    let content_box = gtk::BoxLayout::new(Orientation::Vertical, 0);
    content_box.append(&header_bar);
    content_box.append(&view_stack);

    window.set_child(Some(&content_box));
    window.show();
}

fn get_available_disks() -> Vec<DiskInfo> {
    let mut results = Vec::new();

    let output = Command::new("lsblk")
        .args(["-d", "-n", "-o", "NAME,SIZE,MODEL"])
        .output();

    if let Ok(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            let name = parts[0].to_string();
            if name.starts_with("loop") || name.starts_with("sr") {
                continue;
            }

            let size = if parts.len() > 1 {
                parts[1].to_string()
            } else {
                "Unknown".to_string()
            };

            let model = if parts.len() > 2 {
                parts[2..].join(" ")
            } else {
                "Generic Disk".to_string()
            };

            results.push(DiskInfo { name, model, size });
        }
    }

    if results.is_empty() {
        results.push(DiskInfo {
            name: "sda".to_string(),
            model: "Primary Storage Disk".to_string(),
            size: "256 GB".to_string(),
        });
    }

    results
}
