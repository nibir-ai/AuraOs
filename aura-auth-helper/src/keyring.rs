// keyring.rs — GNOME Keyring Integration
//
// Stores and retrieves OAuth tokens using the secret-service D-Bus API
// (GNOME Keyring / KeePassXC / KDE Wallet via the Secret Service protocol).
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use secret_service::{EncryptionType, SecretService};
use std::collections::HashMap;
use tracing::{debug, info};

/// Store a refresh token in GNOME Keyring
pub async fn store_refresh_token(sub: &str, refresh_token: &str) -> Result<()> {
    let service = SecretService::connect(EncryptionType::Dh)
        .await
        .context("Failed to connect to Secret Service (GNOME Keyring)")?;

    let collection = service
        .get_default_collection()
        .await
        .context("Failed to get default keyring collection")?;

    // Unlock if locked
    if collection.is_locked().await.unwrap_or(true) {
        collection
            .unlock()
            .await
            .context("Failed to unlock keyring")?;
    }

    let label = format!("AuraOS Google Account ({})", sub);

    let mut attributes = HashMap::new();
    attributes.insert("account_id", sub);
    attributes.insert("token_type", "refresh");
    attributes.insert("application", "auraos");

    collection
        .create_item(
            &label,
            attributes,
            refresh_token.as_bytes(),
            true, // replace existing
            "text/plain",
        )
        .await
        .context("Failed to store refresh token in keyring")?;

    info!("Refresh token stored in keyring for sub '{}'", sub);
    Ok(())
}

/// Retrieve a refresh token from GNOME Keyring
pub async fn get_refresh_token(sub: &str) -> Result<String> {
    let service = SecretService::connect(EncryptionType::Dh)
        .await
        .context("Failed to connect to Secret Service")?;

    let collection = service
        .get_default_collection()
        .await
        .context("Failed to get default keyring collection")?;

    if collection.is_locked().await.unwrap_or(true) {
        collection.unlock().await.context("Failed to unlock keyring")?;
    }

    let mut search_attrs = HashMap::new();
    search_attrs.insert("account_id", sub);
    search_attrs.insert("token_type", "refresh");

    let items = collection
        .search_items(search_attrs)
        .await
        .context("Failed to search keyring")?;

    if items.is_empty() {
        anyhow::bail!("No refresh token found for sub '{}'", sub);
    }

    let secret = items[0]
        .get_secret()
        .await
        .context("Failed to read secret from keyring item")?;

    let token = String::from_utf8(secret)
        .context("Refresh token is not valid UTF-8")?;

    debug!("Refresh token retrieved from keyring for sub '{}'", sub);
    Ok(token)
}

/// Cache an access token in the kernel keyring (short-lived, session-scoped)
///
/// Uses the `keyctl` command as a portable interface to the Linux kernel keyring.
/// The token is stored in the user session keyring with a TTL.
pub async fn cache_access_token(sub: &str, access_token: &str, ttl_secs: u64) -> Result<()> {
    let key_name = format!("auraos:access_token:{}", sub);

    // Add key to user session keyring
    let output = tokio::process::Command::new("keyctl")
        .args(["padd", "user", &key_name, "@us"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(access_token.as_bytes()).ok();
            }
            // We need to use the blocking wait since we already spawned
            // For async, we'd need to handle this differently
            Ok(child)
        });

    match output {
        Ok(mut child) => {
            let result = child.wait().await;
            match result {
                Ok(status) if status.success() => {
                    debug!("Access token cached in kernel keyring (TTL={}s)", ttl_secs);

                    // Set timeout on the key
                    // First, find the key ID from stdout
                    // For simplicity, use keyctl timeout with the key description
                    let _ = tokio::process::Command::new("keyctl")
                        .args([
                            "timeout",
                            &format!("%user:{}", key_name),
                            &ttl_secs.to_string(),
                        ])
                        .output()
                        .await;
                }
                _ => {
                    debug!("Failed to cache access token in kernel keyring (keyctl unavailable)");
                    // Non-fatal: kernel keyring is optional
                }
            }
        }
        Err(_) => {
            debug!("keyctl not available — skipping kernel keyring cache");
        }
    }

    Ok(())
}

/// Retrieve an access token from the kernel keyring
pub async fn get_cached_access_token(sub: &str) -> Result<String> {
    let key_name = format!("auraos:access_token:{}", sub);

    let output = tokio::process::Command::new("keyctl")
        .args(["pipe", &format!("%user:{}", key_name)])
        .output()
        .await
        .context("Failed to read from kernel keyring")?;

    if !output.status.success() {
        anyhow::bail!("Access token not found in kernel keyring");
    }

    let token = String::from_utf8(output.stdout)
        .context("Cached access token is not valid UTF-8")?;

    Ok(token)
}
