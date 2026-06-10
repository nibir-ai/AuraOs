// main.rs — AuraOS Google Profile and Contacts Synchronization Service
//
// Periodically queries the Google People API to synchronize user metadata
// (display name, avatar, locale) with GNOME AccountsService.
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use clap::Parser;
use secret_service::{EncryptionType, SecretService};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, error, info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use zbus::{Connection, Proxy};

const CLIENT_ID: &str = "AURAOS_CLIENT_ID_PLACEHOLDER.apps.googleusercontent.com";
const CLIENT_SECRET: &str = "AURAOS_CLIENT_SECRET_PLACEHOLDER";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

#[derive(Parser, Debug)]
#[command(author, version, about = "AuraOS Google Profile Sync Service", long_about = None)]
struct Args {
    /// Path to the accounts database directory
    #[arg(long, default_value = "/var/lib/auraos/accounts")]
    account_db: PathBuf,

    /// Username to sync profile for. If omitted, uses current user.
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
struct PeopleResponse {
    names: Option<Vec<PersonName>>,
    photos: Option<Vec<PersonPhoto>>,
    locales: Option<Vec<PersonLocale>>,
}

#[derive(Debug, Deserialize)]
struct PersonName {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PersonPhoto {
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PersonLocale {
    value: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
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

    info!("Starting profile sync for user '{}'", target_username);

    // Look up account metadata in DB
    let account = lookup_account_by_username(&target_username, &args.account_db)
        .context("Failed to find account metadata")?;

    // Get access token
    let access_token = get_valid_access_token(&target_username, &account.sub).await?;

    // Query Google People API
    info!("Fetching profile details from Google People API...");
    let profile = fetch_google_profile(&access_token).await?;

    // Update AccountsService info via D-Bus
    if let Err(e) = update_accounts_service(&target_username, &profile).await {
        error!("Failed to update AccountsService: {}", e);
    }

    // Download/update profile avatar
    if let Some(ref photo_url) = profile.photo_url {
        info!("Updating profile avatar...");
        if let Err(e) = update_avatar(photo_url, &target_username).await {
            warn!("Failed to update profile avatar: {}", e);
        }
    }

    info!("Profile sync completed successfully for '{}'", target_username);
    Ok(())
}

fn lookup_account_by_username(username: &str, account_db: &Path) -> Result<AccountMetadata> {
    if !account_db.exists() {
        anyhow::bail!("Account database directory '{}' does not exist", account_db.display());
    }

    let dir = fs::read_dir(account_db).context("Failed to read account database directory")?;

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

    anyhow::bail!("No account found matching Linux username '{}'", username)
}

/// Retrieve access token from kernel keyring or fetch a fresh one
async fn get_valid_access_token(username: &str, sub: &str) -> Result<String> {
    // 1. Try reading from kernel keyring
    match get_cached_access_token(sub).await {
        Ok(token) => {
            debug!("Using access token cached in kernel keyring");
            return Ok(token);
        }
        Err(_) => {
            info!("No valid access token found in kernel keyring, attempting refresh...");
        }
    }

    // 2. Fetch refresh token from GNOME Keyring
    let refresh_token = get_refresh_token(sub)
        .await
        .context("Failed to retrieve refresh token from GNOME Keyring")?;

    // 3. Request fresh access token from Google
    let client = reqwest::Client::new();
    let mut params = HashMap::new();
    params.insert("client_id", CLIENT_ID);
    params.insert("client_secret", CLIENT_SECRET);
    params.insert("refresh_token", &refresh_token);
    params.insert("grant_type", "refresh_token");

    let resp = client
        .post(TOKEN_ENDPOINT)
        .form(&params)
        .send()
        .await
        .context("Failed to refresh Google token")?;

    if !resp.status().is_success() {
        anyhow::bail!("Google token endpoint error: {}", resp.text().await.unwrap_or_default());
    }

    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
        expires_in: u64,
        refresh_token: Option<String>,
    }

    let token_resp: TokenResponse = resp.json().await?;

    // Cache the access token in kernel keyring
    let _ = cache_access_token(sub, &token_resp.access_token, token_resp.expires_in).await;

    // Handle token rotation if applicable
    if let Some(ref new_refresh_token) = token_resp.refresh_token {
        let _ = store_refresh_token(sub, new_refresh_token).await;
    }

    Ok(token_resp.access_token)
}

async fn get_cached_access_token(sub: &str) -> Result<String> {
    let key_name = format!("auraos:access_token:{}", sub);
    let output = tokio::process::Command::new("keyctl")
        .args(["pipe", &format!("%user:{}", key_name)])
        .output()
        .await?;

    if !output.status.success() {
        anyhow::bail!("Access token not cached or expired in kernel keyring");
    }

    let token = String::from_utf8(output.stdout)?.trim().to_string();
    Ok(token)
}

async fn cache_access_token(sub: &str, access_token: &str, ttl_secs: u64) -> Result<()> {
    let key_name = format!("auraos:access_token:{}", sub);
    let mut child = tokio::process::Command::new("keyctl")
        .args(["padd", "user", &key_name, "@us"])
        .stdin(std::process::Stdio::piped())
        .spawn()?;

    if let Some(ref mut stdin) = child.stdin {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(access_token.as_bytes()).await?;
    }
    let status = child.wait().await?;
    if status.success() {
        let _ = tokio::process::Command::new("keyctl")
            .args([
                "timeout",
                &format!("%user:{}", key_name),
                &ttl_secs.to_string(),
            ])
            .output()
            .await;
    }
    Ok(())
}

async fn get_refresh_token(sub: &str) -> Result<String> {
    let service = SecretService::connect(EncryptionType::Dh).await?;
    let collection = service.get_default_collection().await?;
    if collection.is_locked().await.unwrap_or(true) {
        collection.unlock().await?;
    }

    let mut search_attrs = HashMap::new();
    search_attrs.insert("account_id", sub);
    search_attrs.insert("token_type", "refresh");

    let items = collection.search_items(search_attrs).await?;
    if items.is_empty() {
        anyhow::bail!("Refresh token not found in keyring");
    }
    let secret = items[0].get_secret().await?;
    let token = String::from_utf8(secret)?.trim().to_string();
    Ok(token)
}

async fn store_refresh_token(sub: &str, refresh_token: &str) -> Result<()> {
    let service = SecretService::connect(EncryptionType::Dh).await?;
    let collection = service.get_default_collection().await?;
    if collection.is_locked().await.unwrap_or(true) {
        collection.unlock().await?;
    }

    let label = format!("AuraOS Google Account ({})", sub);
    let mut attributes = HashMap::new();
    attributes.insert("account_id", sub);
    attributes.insert("token_type", "refresh");
    attributes.insert("application", "auraos");

    collection.create_item(
        &label,
        attributes,
        refresh_token.as_bytes(),
        true,
        "text/plain",
    ).await?;
    Ok(())
}

struct SyncedProfile {
    display_name: Option<String>,
    photo_url: Option<String>,
    locale: Option<String>,
}

async fn fetch_google_profile(access_token: &str) -> Result<SyncedProfile> {
    let client = reqwest::Client::new();
    let url = "https://people.googleapis.com/v1/people/me?personFields=names,photos,locales";
    
    let resp = client
        .get(url)
        .bearer_auth(access_token)
        .send()
        .await
        .context("Failed to connect to Google People API")?;

    if !resp.status().is_success() {
        anyhow::bail!("People API error: {}", resp.text().await.unwrap_or_default());
    }

    let raw: PeopleResponse = resp.json().await?;

    let display_name = raw.names
        .and_then(|ns| ns.first().and_then(|n| n.display_name.clone()));

    // Get the photo URL and convert to higher resolution if possible (e.g. swap s100 for s400)
    let photo_url = raw.photos
        .and_then(|ps| ps.first().and_then(|p| p.url.clone()))
        .map(|url| {
            if url.contains("=s100") {
                url.replace("=s100", "=s400")
            } else {
                url
            }
        });

    let locale = raw.locales
        .and_then(|ls| ls.first().and_then(|l| l.value.clone()));

    Ok(SyncedProfile {
        display_name,
        photo_url,
        locale,
    })
}

/// Download Google photo to ~/.face and ~/.config/auraos/avatar
async fn update_avatar(photo_url: &str, username: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client.get(photo_url).send().await?;

    if !resp.status().is_success() {
        anyhow::bail!("Failed to download avatar: HTTP {}", resp.status());
    }

    let bytes = resp.bytes().await?;
    let home = get_home_dir(username)?;

    let face_path = format!("{}/.face", home);
    fs::write(&face_path, &bytes)?;

    let config_dir = format!("{}/.config/auraos", home);
    fs::create_dir_all(&config_dir).ok();
    let avatar_path = format!("{}/avatar", config_dir);
    fs::write(&avatar_path, &bytes)?;

    Ok(())
}

fn get_home_dir(username: &str) -> Result<String> {
    let output = std::process::Command::new("getent")
        .args(["passwd", username])
        .output()?;
    let line = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = line.trim().split(':').collect();
    if parts.len() >= 6 {
        Ok(parts[5].to_string())
    } else {
        Ok(format!("/home/{}", username))
    }
}

/// Interface to GNOME AccountsService over D-Bus
async fn update_accounts_service(username: &str, profile: &SyncedProfile) -> Result<()> {
    let connection = Connection::system().await.context("Failed to connect to D-Bus System Bus")?;

    // Find the UID of the target user
    let id_output = std::process::Command::new("id")
        .args(["-u", username])
        .output()?;
    let uid_str = String::from_utf8(id_output.stdout)?.trim().to_string();
    if uid_str.is_empty() {
        anyhow::bail!("Could not retrieve UID for user '{}'", username);
    }

    let path = format!("/org/freedesktop/Accounts/User{}", uid_str);
    let proxy = Proxy::new(
        &connection,
        "org.freedesktop.Accounts",
        &path,
        "org.freedesktop.Accounts.User",
    ).await?;

    if let Some(ref display_name) = profile.display_name {
        info!("Updating RealName in AccountsService to '{}'...", display_name);
        match proxy.call::<_, _, ()>("SetRealName", &(display_name,)).await {
            Ok(_) => info!("SetRealName succeeded"),
            Err(e) => warn!("SetRealName failed (possibly insufficient D-Bus permissions): {}", e),
        }
    }

    if let Some(ref locale) = profile.locale {
        let lang = format!("{}.UTF-8", locale);
        info!("Updating Language in AccountsService to '{}'...", lang);
        match proxy.call::<_, _, ()>("SetLanguage", &(lang.as_str(),)).await {
            Ok(_) => info!("SetLanguage succeeded"),
            Err(e) => warn!("SetLanguage failed: {}", e),
        }
    }

    // Update avatar path icon in AccountsService
    let home = get_home_dir(username)?;
    let face_file = format!("{}/.face", home);
    if Path::new(&face_file).exists() {
        match proxy.call::<_, _, ()>("SetIconFile", &(face_file.as_str(),)).await {
            Ok(_) => info!("SetIconFile succeeded"),
            Err(e) => warn!("SetIconFile failed: {}", e),
        }
    }

    Ok(())
}
