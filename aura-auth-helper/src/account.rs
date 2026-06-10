// account.rs — Linux User Account Management
//
// Handles creating Linux user accounts mapped to Google identities,
// writing account metadata, downloading avatars, and managing offline PINs.
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use tracing::{debug, info, warn};

use crate::oauth::{GoogleProfile, TokenResponse};

/// Account metadata stored in /var/lib/auraos/accounts/<sub>.json
#[derive(Debug, Serialize, Deserialize)]
pub struct AccountMetadata {
    pub sub: String,
    pub email: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub linux_username: String,
    pub scopes_granted: Vec<String>,
    pub account_created: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offline_pin_hash: Option<String>,
}

/// Create a new Linux user account for a Google profile.
///
/// The username is derived from the email prefix, sanitized for Linux
/// username requirements (lowercase, alphanumeric + underscore, max 32 chars).
pub fn create_user_account(profile: &GoogleProfile, account_db: &str) -> Result<String> {
    let username = sanitize_username(&profile.email);

    // Check if the user already exists
    if user_exists(&username) {
        info!("Linux user '{}' already exists, skipping creation", username);
        return Ok(username);
    }

    // Generate a home directory path based on a hash of the Google sub
    let sub_hash = {
        let mut hasher = Sha256::new();
        hasher.update(profile.sub.as_bytes());
        let hash = hasher.finalize();
        hex::encode(&hash[..8]) // First 8 bytes = 16 hex chars
    };
    let home_dir = format!("/home/google_{}", sub_hash);

    // Create the user via useradd
    info!("Creating Linux user '{}' with home '{}'", username, home_dir);

    let output = Command::new("useradd")
        .args([
            "--create-home",
            "--home-dir", &home_dir,
            "--gid", "auraos-users",
            "--shell", "/bin/bash",
            "--comment", &profile.name,
            &username,
        ])
        .output()
        .context("Failed to execute useradd")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        // If the group doesn't exist yet, try without --gid
        if stderr.contains("group") && stderr.contains("does not exist") {
            warn!("Group 'auraos-users' not found, creating user without group");

            let output = Command::new("useradd")
                .args([
                    "--create-home",
                    "--home-dir", &home_dir,
                    "--shell", "/bin/bash",
                    "--comment", &profile.name,
                    &username,
                ])
                .output()
                .context("Failed to execute useradd (retry without group)")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("useradd failed: {}", stderr);
            }
        } else {
            anyhow::bail!("useradd failed: {}", stderr);
        }
    }

    // Ensure the account database directory exists
    fs::create_dir_all(account_db)
        .context("Failed to create account database directory")?;

    // Set secure permissions on account DB
    let perms = fs::Permissions::from_mode(0o700);
    fs::set_permissions(account_db, perms).ok();

    Ok(username)
}

/// Write account metadata to /var/lib/auraos/accounts/<sub>.json
pub fn write_account_metadata(
    profile: &GoogleProfile,
    linux_username: &str,
    tokens: &TokenResponse,
    account_db: &str,
) -> Result<()> {
    let metadata = AccountMetadata {
        sub: profile.sub.clone(),
        email: profile.email.clone(),
        display_name: profile.name.clone(),
        avatar_url: profile.picture.clone(),
        linux_username: linux_username.to_string(),
        scopes_granted: tokens.scope.split(' ').map(String::from).collect(),
        account_created: chrono_now_iso8601(),
        offline_pin_hash: None,
    };

    let json = serde_json::to_string_pretty(&metadata)
        .context("Failed to serialize account metadata")?;

    let path = format!("{}/{}.json", account_db, profile.sub);
    let tmp_path = format!("{}.tmp", path);

    // Write atomically: write to .tmp, then rename
    fs::write(&tmp_path, &json)
        .context("Failed to write account metadata")?;

    // Set permissions to 600 (owner-only)
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(&tmp_path, perms)
        .context("Failed to set account metadata permissions")?;

    fs::rename(&tmp_path, &path)
        .context("Failed to rename account metadata")?;

    debug!("Account metadata written to {}", path);
    Ok(())
}

/// Download the user's Google profile avatar
pub async fn download_avatar(url: &str, linux_username: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client.get(url).send().await?;

    if !resp.status().is_success() {
        anyhow::bail!("Failed to download avatar (HTTP {})", resp.status());
    }

    let bytes = resp.bytes().await?;

    // Get the user's home directory
    let home = get_home_dir(linux_username)?;

    // Write as ~/.face (GNOME AccountsService standard)
    let face_path = format!("{}/.face", home);
    fs::write(&face_path, &bytes)
        .context("Failed to write ~/.face")?;

    // Also write to AuraOS config dir
    let config_dir = format!("{}/.config/auraos", home);
    fs::create_dir_all(&config_dir).ok();
    let avatar_path = format!("{}/avatar", config_dir);
    fs::write(&avatar_path, &bytes)
        .context("Failed to write avatar")?;

    debug!("Avatar downloaded to {} and {}", face_path, avatar_path);
    Ok(())
}

/// Look up an account by Linux username
pub fn lookup_account_by_username(username: &str, account_db: &str) -> Result<AccountMetadata> {
    let dir = fs::read_dir(account_db)
        .context("Failed to read account database directory")?;

    for entry in dir {
        let entry = entry?;
        let path = entry.path();

        if path.extension().map_or(false, |ext| ext == "json") {
            let contents = fs::read_to_string(&path)?;
            let account: AccountMetadata = serde_json::from_str(&contents)?;

            if account.linux_username == username {
                return Ok(account);
            }
        }
    }

    anyhow::bail!("No account found for username '{}'", username)
}

/// Set an offline PIN for an account
pub fn set_offline_pin(username: &str, pin: &str, account_db: &str) -> Result<()> {
    let mut account = lookup_account_by_username(username, account_db)?;

    // Hash the PIN using bcrypt
    let hash = bcrypt_hash(pin)?;
    account.offline_pin_hash = Some(hash);

    // Write updated account metadata
    let json = serde_json::to_string_pretty(&account)?;
    let path = format!("{}/{}.json", account_db, account.sub);

    fs::write(&path, json)?;
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(&path, perms)?;

    info!("Offline PIN set for user '{}'", username);
    Ok(())
}

// ─── Internal Helpers ────────────────────────────────────────────────

/// Sanitize an email prefix into a valid Linux username
fn sanitize_username(email: &str) -> String {
    let prefix = email.split('@').next().unwrap_or("user");

    let sanitized: String = prefix
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '.' || *c == '-')
        .collect::<String>()
        .to_lowercase();

    // Ensure it starts with a letter
    let username = if sanitized.starts_with(|c: char| c.is_alphabetic()) {
        sanitized
    } else {
        format!("u{}", sanitized)
    };

    // Truncate to 32 characters (Linux limit)
    username[..username.len().min(32)].to_string()
}

/// Check if a Linux user already exists
fn user_exists(username: &str) -> bool {
    Command::new("id")
        .arg(username)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get a user's home directory from /etc/passwd
fn get_home_dir(username: &str) -> Result<String> {
    let output = Command::new("getent")
        .args(["passwd", username])
        .output()
        .context("Failed to get user's home directory")?;

    let line = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = line.trim().split(':').collect();

    if parts.len() >= 6 {
        Ok(parts[5].to_string())
    } else {
        // Fallback
        Ok(format!("/home/{}", username))
    }
}

/// Get current UTC time in ISO 8601 format
fn chrono_now_iso8601() -> String {
    // Using command-line fallback to avoid adding chrono as a dependency
    let output = Command::new("date")
        .arg("--utc")
        .arg("+%Y-%m-%dT%H:%M:%SZ")
        .output();

    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        _ => "1970-01-01T00:00:00Z".to_string(),
    }
}

/// Hash a PIN using bcrypt (via the `bcrypt` command or library)
fn bcrypt_hash(pin: &str) -> Result<String> {
    // Use Python's bcrypt as a portable fallback
    let output = Command::new("python3")
        .args([
            "-c",
            &format!(
                "import bcrypt; print(bcrypt.hashpw(b'{}', bcrypt.gensalt(rounds=12)).decode())",
                pin.replace('\'', "\\'")
            ),
        ])
        .output()
        .context("Failed to hash PIN (python3 with bcrypt required)")?;

    if !output.status.success() {
        anyhow::bail!("bcrypt hashing failed");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_username() {
        assert_eq!(sanitize_username("john.doe@gmail.com"), "john.doe");
        assert_eq!(sanitize_username("Alice.Smith@example.com"), "alice.smith");
        assert_eq!(sanitize_username("123user@test.com"), "u123user");
        assert_eq!(sanitize_username("user+tag@gmail.com"), "usertag");
    }

    #[test]
    fn test_sanitize_username_length() {
        let long_email = "verylongusernamethatexceedsthelimit@example.com";
        let username = sanitize_username(long_email);
        assert!(username.len() <= 32);
    }
}
