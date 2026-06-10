// confirmation.rs — Desktop Confirmation Dialogs
//
// Shows confirmation dialogs for destructive tool actions before Gemini
// can execute them. Uses GNOME desktop notifications with action buttons.
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use tracing::{debug, info, warn};

/// Desktop confirmation dialog for destructive tool actions
pub struct DesktopConfirmation;

impl DesktopConfirmation {
    /// Show a confirmation dialog and wait for user response.
    ///
    /// Uses zenity (GTK dialog) for modal confirmation on GNOME.
    /// Returns true if the user approved, false if denied.
    pub async fn show(title: &str, description: &str) -> bool {
        info!("Requesting user confirmation: {}", title);

        // Try zenity first (GNOME native)
        let result = tokio::process::Command::new("zenity")
            .args([
                "--question",
                "--title", title,
                "--text", description,
                "--ok-label", "Allow",
                "--cancel-label", "Deny",
                "--width", "400",
                "--icon-name", "dialog-warning",
            ])
            .output()
            .await;

        match result {
            Ok(output) => {
                let approved = output.status.success();
                if approved {
                    info!("User approved action: {}", title);
                } else {
                    warn!("User denied action: {}", title);
                }
                approved
            }
            Err(e) => {
                // Zenity not available — try kdialog as fallback
                debug!("zenity not available ({}), trying kdialog", e);

                let result = tokio::process::Command::new("kdialog")
                    .args([
                        "--warningyesno",
                        description,
                        "--title", title,
                    ])
                    .output()
                    .await;

                match result {
                    Ok(output) => output.status.success(),
                    Err(_) => {
                        // No dialog tool available — deny by default for safety
                        warn!("No confirmation dialog available — denying action for safety");
                        false
                    }
                }
            }
        }
    }
}
