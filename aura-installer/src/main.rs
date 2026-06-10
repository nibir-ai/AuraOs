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
        .default_width(680)
        .default_height(480)
        .resizable(false)
        .build();

    let view_stack = ViewStack::new();
    view_stack.set_transition_type(adw::ViewStackTransitionType::SlideLeftRight);

    // State container
    let selected_disk: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    // Page 1: Welcome Page
    let welcome_box = gtk::BoxLayout::builder()
        .orientation(Orientation::Vertical)
        .spacing(20)
        .margin_top(40)
        .margin_bottom(40)
        .margin_start(40)
        .margin_end(40)
        .valign(Align::Center)
        .build();

    let welcome_title = Label::builder()
        .label("Welcome to AuraOS")
        .css_classes(vec!["title-1", "bold"])
        .build();
    welcome_box.append(&welcome_title);

    let welcome_desc = Label::builder()
        .label("This wizard will install AuraOS on your computer. You will need a Google account and an active internet connection to complete setup.")
        .wrap(true)
        .justify(gtk::Justification::Center)
        .build();
    welcome_box.append(&welcome_desc);

    let start_btn = Button::builder()
        .label("Begin Installation")
        .css_classes(vec!["suggested-action", "pill"])
        .halign(Align::Center)
        .margin_top(30)
        .build();
    welcome_box.append(&start_btn);

    view_stack.add_titled(&welcome_box, Some("welcome"), "Welcome");

    // Page 2: Disk Selection Page
    let disk_box = gtk::BoxLayout::builder()
        .orientation(Orientation::Vertical)
        .spacing(16)
        .margin_top(30)
        .margin_bottom(30)
        .margin_start(45)
        .margin_end(45)
        .build();

    let disk_title = Label::builder()
        .label("Select Installation Disk")
        .css_classes(vec!["title-2", "bold"])
        .build();
    disk_box.append(&disk_title);

    let disk_warning = Label::builder()
        .label("WARNING: The selected disk will be completely formatted. Back up your data before proceeding.")
        .wrap(true)
        .css_classes(vec!["error"])
        .build();
    disk_box.append(&disk_warning);

    let list_box = ListBox::new();
    list_box.set_selection_mode(SelectionMode::Single);
    list_box.set_margin_top(10);
    list_box.set_margin_bottom(10);

    // Load available disks via lsblk
    let disks = get_available_disks();
    for disk in &disks {
        let row = ListBoxRow::new();
        let row_box = gtk::BoxLayout::new(Orientation::Horizontal, 12);
        row_box.set_margin_top(10);
        row_box.set_margin_bottom(10);
        row_box.set_margin_start(10);
        row_box.set_margin_end(10);

        let icon = gtk::Image::from_icon_name("drive-harddisk");
        row_box.append(&icon);

        let details_box = gtk::BoxLayout::new(Orientation::Vertical, 2);
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
        .min_content_height(160)
        .build();
    disk_box.append(&scroll);

    let disk_next_btn = Button::builder()
        .label("Next Step")
        .css_classes(vec!["suggested-action", "pill"])
        .halign(Align::Center)
        .sensitive(false)
        .build();
    disk_box.append(&disk_next_btn);

    view_stack.add_titled(&disk_box, Some("disk"), "Select Disk");

    // Connect disk list selection changes
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

    // Page 3: Google Account Sign-In Page
    let login_box = gtk::BoxLayout::builder()
        .orientation(Orientation::Vertical)
        .spacing(20)
        .margin_top(40)
        .margin_bottom(40)
        .margin_start(40)
        .margin_end(40)
        .valign(Align::Center)
        .build();

    let login_title = Label::builder()
        .label("Connect Google Account")
        .css_classes(vec!["title-1", "bold"])
        .build();
    login_box.append(&login_title);

    let login_desc = Label::builder()
        .label("AuraOS operates on your Google Identity. Signing in now will configure your Linux user environment, download your profile details, and cache tokens for offline access.")
        .wrap(true)
        .justify(gtk::Justification::Center)
        .build();
    login_box.append(&login_desc);

    let login_btn = Button::builder()
        .label("Sign in with Google")
        .css_classes(vec!["suggested-action", "pill"])
        .halign(Align::Center)
        .margin_top(20)
        .build();
    login_box.append(&login_btn);

    let login_next_btn = Button::builder()
        .label("Begin File Copy")
        .css_classes(vec!["pill"])
        .halign(Align::Center)
        .margin_top(10)
        .sensitive(false)
        .build();
    login_box.append(&login_next_btn);

    view_stack.add_titled(&login_box, Some("login"), "Log In");

    // Trigger auth helper on sign-in button click
    let login_next_btn_clone = login_next_btn.clone();
    login_btn.connect_clicked(move |_| {
        // Run auth-helper synchronously in the live system
        let status = Command::new("pkexec")
            .arg("/usr/lib/auraos/aura-auth-helper")
            .status();

        match status {
            Ok(s) if s.success() => {
                login_next_btn_clone.set_sensitive(true);
                login_next_btn_clone.set_css_classes(&["suggested-action", "pill"]);
            }
            _ => {
                // If it fails or is canceled, show warning
                eprintln!("Authentication canceled or failed");
            }
        }
    });

    // Page 4: Progress Page
    let progress_box = gtk::BoxLayout::builder()
        .orientation(Orientation::Vertical)
        .spacing(24)
        .margin_top(50)
        .margin_bottom(50)
        .margin_start(50)
        .margin_end(50)
        .valign(Align::Center)
        .build();

    let progress_title = Label::builder()
        .label("Installing AuraOS")
        .css_classes(vec!["title-1", "bold"])
        .build();
    progress_box.append(&progress_title);

    let progress_bar = ProgressBar::new();
    progress_bar.set_show_text(true);
    progress_box.append(&progress_bar);

    let progress_status = Label::builder()
        .label("Preparing target disk drive...")
        .wrap(true)
        .build();
    progress_box.append(&progress_status);

    view_stack.add_titled(&progress_box, Some("progress"), "Progress");

    // Page 5: Finished Page
    let finished_box = gtk::BoxLayout::builder()
        .orientation(Orientation::Vertical)
        .spacing(20)
        .margin_top(40)
        .margin_bottom(40)
        .margin_start(40)
        .margin_end(40)
        .valign(Align::Center)
        .build();

    let finished_icon = gtk::Image::from_icon_name("object-select-symbolic");
    finished_icon.set_pixel_size(64);
    finished_box.append(&finished_icon);

    let finished_title = Label::builder()
        .label("Installation Complete")
        .css_classes(vec!["title-1", "bold"])
        .build();
    finished_box.append(&finished_title);

    let finished_desc = Label::builder()
        .label("AuraOS is fully installed. You can now restart your PC and log in using your Google account.")
        .wrap(true)
        .justify(gtk::Justification::Center)
        .build();
    finished_box.append(&finished_desc);

    let reboot_btn = Button::builder()
        .label("Reboot System")
        .css_classes(vec!["suggested-action", "pill"])
        .halign(Align::Center)
        .margin_top(20)
        .build();
    finished_box.append(&reboot_btn);

    view_stack.add_titled(&finished_box, Some("finished"), "Finished");

    // Page navigation transitions
    let view_stack_clone = view_stack.clone();
    start_btn.connect_clicked(move |_| {
        view_stack_clone.set_visible_child_name("disk");
    });

    let view_stack_clone = view_stack.clone();
    disk_next_btn.connect_clicked(move |_| {
        view_stack_clone.set_visible_child_name("login");
    });

    // Begin installation button click (login next button)
    let view_stack_clone = view_stack.clone();
    let selected_disk_clone = selected_disk.clone();
    let progress_bar_clone = progress_bar.clone();
    let progress_status_clone = progress_status.clone();
    login_next_btn.connect_clicked(move |_| {
        view_stack_clone.set_visible_child_name("progress");

        let disk_path = selected_disk_clone.borrow().clone().unwrap_or_default();
        let config = InstallConfig { disk: disk_path };

        // Channel to pass progress from thread back to GTK main context
        let (tx, rx) = glib::MainContext::channel::<(f64, String)>(glib::Priority::default());

        let p_bar = progress_bar_clone.clone();
        let p_status = progress_status_clone.clone();
        let vs = view_stack_clone.clone();

        // Handle incoming channel messages
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

        // Run install loop in an async/OS thread
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
                    let _ = tx.send((1.0, "Success".to_string()));
                }
                Err(e) => {
                    let _ = tx.send((0.0, format!("Error occurred during install: {}", e)));
                }
            }
        });
    });

    // Reboot button triggers standard reboot
    reboot_btn.connect_clicked(|_| {
        let _ = Command::new("systemctl").arg("reboot").status();
    });

    // Window Layout Structure
    let header_bar = HeaderBar::new();
    let content_box = gtk::BoxLayout::new(Orientation::Vertical, 0);
    content_box.append(&header_bar);
    content_box.append(&view_stack);

    window.set_child(Some(&content_box));
    window.show();
}

/// Retrieve block storage disks using lsblk (filters loop, CD, virtual partitions)
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
            // Skip loop devices and cdroms
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
        // Fallback mock disk for simulation
        results.push(DiskInfo {
            name: "sda".to_string(),
            model: "Mock System Disk".to_string(),
            size: "128 GB".to_string(),
        });
    }

    results
}
