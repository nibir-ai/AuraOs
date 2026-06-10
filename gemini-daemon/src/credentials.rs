// credentials.rs — Credential Store for Gemini Daemon
//
// Manages access to OAuth tokens via GNOME Keyring and kernel keyring.
// Provides per-scope token pools as described in the blueprint.
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use secret_service::{EncryptionType, SecretService};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Cached token entry
struct TokenEntry {
    access_token: String,
    expires_at: std::time::Instant,
}

/// Credential store backed by GNOME Keyring and kernel keyring
pub struct CredentialStore {
    /// Cached access tokens per scope
    token_cache: Arc<RwLock<HashMap<String, TokenEntry>>>,
    /// Current user's Google sub (user ID)
    current_sub: Arc<RwLock<Option<String>>>,
}

impl CredentialStore {
    /// Connect to the credential stores
    pub async fn connect() -> Result<Self> {
        // Try to determine the current user's Google sub from account metadata
        let current_sub = Self::detect_current_user().await;

        Ok(Self {
            token_cache: Arc::new(RwLock::new(HashMap::new())),
            current_sub: Arc::new(RwLock::new(current_sub)),
        })
    }

    /// Get the current user's access token (from cache or kernel keyring)
    pub async fn get_current_access_token(&self) -> Result<String> {
        // Check cache first
        {
            let cache = self.token_cache.read().await;
            if let Some(entry) = cache.get("default") {
                if entry.expires_at > std::time::Instant::now() {
                    debug!("Access token from cache (valid)");
                    return Ok(entry.access_token.clone());
                }
            }
        }

        // Try kernel keyring
        let sub = self.get_current_sub().await?;
        let key_name = format!("auraos:access_token:{}", sub);

        let output = tokio::process::Command::new("keyctl")
            .args(["pipe", &format!("%user:{}", key_name)])
            .output()
            .await;

        if let Ok(output) = output {
            if output.status.success() {
                let token = String::from_utf8(output.stdout)
                    .context("Token not valid UTF-8")?;

                if !token.is_empty() {
                    // Cache it
                    let mut cache = self.token_cache.write().await;
                    cache.insert("default".to_string(), TokenEntry {
                        access_token: token.clone(),
                        expires_at: std::time::Instant::now() + std::time::Duration::from_secs(300),
                    });

                    debug!("Access token from kernel keyring");
                    return Ok(token);
                }
            }
        }

        // Last resort: try to refresh via the refresh token
        self.refresh_access_token(&sub).await
    }

    /// Get a scoped access token for a specific Google API
    pub async fn get_scoped_token(&self, scope: &str) -> Result<String> {
        // For now, return the default token
        // In a full implementation, this would maintain separate token pools
        // per scope for least-privilege access
        self.get_current_access_token().await
    }

    /// Get the current user's Google sub (user ID)
    async fn get_current_sub(&self) -> Result<String> {
        let sub = self.current_sub.read().await;
        sub.clone()
            .ok_or_else(|| anyhow::anyhow!("No current user — not authenticated"))
    }

    /// Detect the current user from account metadata files
    async fn detect_current_user() -> Option<String> {
        let account_dir = "/var/lib/auraos/accounts";

        // Get the current Linux user's username
        let output = std::process::Command::new("whoami")
            .output()
            .ok()?;

        let username = String::from_utf8(output.stdout).ok()?.trim().to_string();

        // Scan account metadata files for matching username
        let dir = std::fs::read_dir(account_dir).ok()?;

        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "json") {
                if let Ok(contents) = std::fs::read_to_string(&path) {
                    if let Ok(account) = serde_json::from_str::<serde_json::Value>(&contents) {
                        if account.get("linux_username")
                            .and_then(|u| u.as_str())
                            .map_or(false, |u| u == username)
                        {
                            return account.get("sub")
                                .and_then(|s| s.as_str())
                                .map(|s| s.to_string());
                        }
                    }
                }
            }
        }

        warn!("Could not detect current user's Google sub");
        None
    }

    /// Refresh the access token using the stored refresh token
    async fn refresh_access_token(&self, sub: &str) -> Result<String> {
        // Get refresh token from GNOME Keyring
        let service = SecretService::connect(EncryptionType::Dh)
            .await
            .context("Failed to connect to Secret Service")?;

        let collection = service
            .get_default_collection()
            .await
            .context("Failed to get default collection")?;

        if collection.is_locked().await.unwrap_or(true) {
            collection.unlock().await.context("Failed to unlock keyring")?;
        }

        let mut attrs = HashMap::new();
        attrs.insert("account_id", sub);
        attrs.insert("token_type", "refresh");

        let items = collection
            .search_items(attrs)
            .await
            .context("Failed to search keyring")?;

        if items.is_empty() {
            anyhow::bail!("No refresh token found in keyring for sub '{}'", sub);
        }

        let refresh_token = String::from_utf8(
            items[0].get_secret().await.context("Failed to read secret")?
        ).context("Refresh token not UTF-8")?;

        // Exchange refresh token for new access token
        let client = reqwest::Client::new();
        let resp = client
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("client_id", "AURAOS_CLIENT_ID_PLACEHOLDER.apps.googleusercontent.com"),
                ("client_secret", "AURAOS_CLIENT_SECRET_PLACEHOLDER"),
                ("refresh_token", &refresh_token),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await
            .context("Token refresh request failed")?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Token refresh failed: {}", body);
        }

        #[derive(serde::Deserialize)]
        struct RefreshResponse {
            access_token: String,
            expires_in: u64,
        }

        let token_resp: RefreshResponse = resp.json().await?;

        // Cache the new token
        let mut cache = self.token_cache.write().await;
        cache.insert("default".to_string(), TokenEntry {
            access_token: token_resp.access_token.clone(),
            expires_at: std::time::Instant::now()
                + std::time::Duration::from_secs(token_resp.expires_in),
        });

        // Also store in kernel keyring
        let key_name = format!("auraos:access_token:{}", sub);
        let _ = tokio::process::Command::new("keyctl")
            .args(["padd", "user", &key_name, "@us"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(ref mut stdin) = child.stdin {
                    stdin.write_all(token_resp.access_token.as_bytes()).ok();
                }
                Ok(child)
            });

        Ok(token_resp.access_token)
    }
}
