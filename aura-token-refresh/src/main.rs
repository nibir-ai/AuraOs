// main.rs — AuraOS Google Token Refresh Service
//
// Periodically refreshes OAuth 2.0 access tokens and handles token rotation,
// storing the results back in GNOME Keyring and caching them in the kernel keyring.
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use clap::Parser;
use secret_service::{EncryptionType, SecretService};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::{debug, error, info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

const CLIENT_ID: &str = "AURAOS_CLIENT_ID_PLACEHOLDER.apps.googleusercontent.com";
const CLIENT_SECRET: &str = "AURAOS_CLIENT_SECRET_PLACEHOLDER";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const AUDIT_LOG_PATH: &str = "/var/log/auraos/token-lifecycle.log";

#[derive(Parser, Debug)]
#[command(author, version, about = "AuraOS Google OAuth Token Refresh Service", long_about = None)]
struct Args {
    /// Path to the accounts database directory
    #[arg(long, default_value = "/var/lib/auraos/accounts")]
    account_db: PathBuf,

    /// Username to refresh tokens for. If omitted, uses current user.
    #[arg(long)]
    username: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccountMetadata {
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

#[derive(Debug, Deserialize)]
struct TokenRefreshResponse {
    access_token: String,
    expires_in: u64,
    refresh_token: Option<String>,
    scope: Option<String>,
    token_type: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to standard error
    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let args = Args::parse();

    // Determine target username
    let target_username = match args.username {
        Some(u) => u,
        None => std::env::var("USER").context("Could not determine current username via USER environment variable")?,
    };

    info!("Starting token refresh for user '{}'", target_username);

    // Look up account metadata in DB
    let account = lookup_account_by_username(&target_username, &args.account_db)
        .context("Failed to find account metadata")?;

    info!("Found Google account profile: sub={}, email={}", account.sub, account.email);

    // Retrieve refresh token from GNOME Keyring
    let refresh_token = get_refresh_token(&account.sub)
        .await
        .context("Failed to retrieve refresh token from GNOME Keyring")?;

    // Refresh access token via Google endpoint
    info!("Requesting fresh access token from Google...");
    let token_resp = refresh_oauth_token(&refresh_token).await?;
    info!("Successfully received fresh access token");

    // Cache the access token in the kernel keyring
    if let Err(e) = cache_access_token(&account.sub, &token_resp.access_token, token_resp.expires_in).await {
        warn!("Failed to cache access token in kernel keyring: {}", e);
    } else {
        info!("Cached access token in kernel keyring");
    }

    // Handle token rotation (if Google returned a new refresh token)
    if let Some(ref new_refresh_token) = token_resp.refresh_token {
        info!("Token rotation detected. Storing new refresh token in GNOME Keyring...");
        if let Err(e) = store_refresh_token(&account.sub, new_refresh_token).await {
            error!("Failed to store rotated refresh token: {}", e);
            write_audit_log(&target_username, &account.sub, "ROTATION_FAILURE", &format!("Failed to store rotated refresh token: {}", e));
            anyhow::bail!("Failed to store rotated refresh token in keyring");
        } else {
            info!("Stored rotated refresh token successfully");
            write_audit_log(&target_username, &account.sub, "ROTATED", "OAuth refresh token rotated and stored successfully");
        }
    } else {
        write_audit_log(&target_username, &account.sub, "REFRESHED", "OAuth access token refreshed successfully");
    }

    info!("Token refresh sequence completed successfully for '{}'", target_username);
    Ok(())
}

/// Lookup account metadata by Linux username in account_db directory
fn lookup_account_by_username(username: &str, account_db: &Path) -> Result<AccountMetadata> {
    if !account_db.exists() {
        anyhow::bail!("Account database directory '{}' does not exist", account_db.display());
    }

    let dir = fs::read_dir(account_db).context("Failed to read account database directory")?;

    for entry in dir {
        let entry = entry?;
        let path = entry.path();

        if path.extension().map_or(false, |ext| ext == "json") {
            let contents = fs::read_to_string(&path)
                .context(format!("Failed to read metadata file: {}", path.display()))?;
            let account: AccountMetadata = serde_json::from_str(&contents)
                .context(format!("Failed to parse metadata file: {}", path.display()))?;

            if account.linux_username == username {
                return Ok(account);
            }
        }
    }

    anyhow::bail!("No account found matching Linux username '{}'", username)
}

/// Retrieve refresh token from default GNOME Keyring collection
async fn get_refresh_token(sub: &str) -> Result<String> {
    let service = SecretService::connect(EncryptionType::Dh)
        .await
        .context("Failed to connect to Secret Service")?;

    let collection = service
        .get_default_collection()
        .await
        .context("Failed to get default keyring collection")?;

    if collection.is_locked().await.unwrap_or(true) {
        collection.unlock().await.context("Failed to unlock default keyring collection")?;
    }

    let mut search_attrs = HashMap::new();
    search_attrs.insert("account_id", sub);
    search_attrs.insert("token_type", "refresh");

    let items = collection
        .search_items(search_attrs)
        .await
        .context("Failed to search default keyring collection")?;

    if items.is_empty() {
        anyhow::bail!("No refresh token item found in keyring");
    }

    let secret = items[0]
        .get_secret()
        .await
        .context("Failed to retrieve secret from keyring item")?;

    let token = String::from_utf8(secret)
        .context("Refresh token is not valid UTF-8 string")?;

    Ok(token)
}

/// Store refreshed or rotated refresh token back to keyring
async fn store_refresh_token(sub: &str, refresh_token: &str) -> Result<()> {
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
        .context("Failed to write token item to keyring")?;

    Ok(())
}

/// Request a new access token from Google OAuth endpoint
async fn refresh_oauth_token(refresh_token: &str) -> Result<TokenRefreshResponse> {
    let client = reqwest::Client::builder()
        .build()
        .context("Failed to build HTTP client")?;

    let mut params = HashMap::new();
    params.insert("client_id", CLIENT_ID);
    params.insert("client_secret", CLIENT_SECRET);
    params.insert("refresh_token", refresh_token);
    params.insert("grant_type", "refresh_token");

    let resp = client
        .post(TOKEN_ENDPOINT)
        .form(&params)
        .send()
        .await
        .context("Failed to send token refresh request")?;

    if !resp.status().is_success() {
        let err_text = resp.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        anyhow::bail!("Google token endpoint returned error: {}", err_text);
    }

    let token_resp: TokenRefreshResponse = resp
        .json()
        .await
        .context("Failed to deserialize token refresh response JSON")?;

    Ok(token_resp)
}

/// Cache the refreshed access token into the Linux kernel session keyring
async fn cache_access_token(sub: &str, access_token: &str, ttl_secs: u64) -> Result<()> {
    let key_name = format!("auraos:access_token:{}", sub);

    // Write to keyctl padd
    let output = tokio::process::Command::new("keyctl")
        .args(["padd", "user", &key_name, "@us"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(access_token.as_bytes());
            }
            Ok(child)
        });

    match output {
        Ok(mut child) => {
            let status = child.wait().await?;
            if status.success() {
                // Set timeout
                let _ = tokio::process::Command::new("keyctl")
                    .args([
                        "timeout",
                        &format!("%user:{}", key_name),
                        &ttl_secs.to_string(),
                    ])
                    .output()
                    .await;
                Ok(())
            } else {
                anyhow::bail!("keyctl execution failed to store key");
            }
        }
        Err(e) => {
            anyhow::bail!("keyctl process launch failed: {}", e);
        }
    }
}

/// Write an audit log entry to /var/log/auraos/token-lifecycle.log
fn write_audit_log(username: &str, sub: &str, event_type: &str, message: &str) {
    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let log_line = format!("[{}] USER={} SUB={} EVENT={} : {}\n", timestamp, username, sub, event_type, message);

    // Attempt to open and append to the log file
    let path = Path::new(AUDIT_LOG_PATH);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    match OpenOptions::new().create(true).append(true).open(path) {
        Ok(mut file) => {
            if let Err(e) = file.write_all(log_line.as_bytes()) {
                warn!("Failed to write to audit log file: {}", e);
            }
        }
        Err(e) => {
            warn!("Could not open audit log file '{}': {}. Logging to console instead: {}", AUDIT_LOG_PATH, e, log_line.trim());
        }
    }
}
