// oauth.rs — OAuth 2.0 PKCE Flow Implementation
//
// Implements the OAuth 2.0 Authorization Code flow with PKCE for
// Google sign-in. Handles code verifier generation, challenge derivation,
// local redirect server, and token exchange.
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use sha2::{Digest, Sha256};
use serde::Deserialize;
use std::collections::HashMap;
use std::net::TcpListener;
use tokio::sync::oneshot;
use tracing::{debug, error};

use crate::config;

/// PKCE challenge parameters
pub struct PkceChallenge {
    /// Random code verifier (high-entropy, base64url-encoded)
    pub code_verifier: String,
    /// SHA256 hash of code_verifier, base64url-encoded
    pub code_challenge: String,
    /// Random state parameter for CSRF protection
    pub state: String,
}

impl PkceChallenge {
    /// Generate a new PKCE challenge with 256-bit entropy
    pub fn generate() -> Self {
        // Generate 32 bytes (256 bits) of cryptographic randomness
        let mut verifier_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut verifier_bytes);
        let code_verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);

        // Derive the challenge: BASE64URL(SHA256(code_verifier))
        let mut hasher = Sha256::new();
        hasher.update(code_verifier.as_bytes());
        let hash = hasher.finalize();
        let code_challenge = URL_SAFE_NO_PAD.encode(hash);

        // Generate random state for CSRF protection
        let mut state_bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut state_bytes);
        let state = URL_SAFE_NO_PAD.encode(state_bytes);

        Self {
            code_verifier,
            code_challenge,
            state,
        }
    }
}

/// Data received from Google's OAuth redirect callback
pub struct AuthCallback {
    /// The authorization code from Google
    pub code: String,
    /// The state parameter (must match our generated state)
    pub state: String,
}

/// Tokens received from Google's token exchange endpoint
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub expires_in: u64,
    pub scope: String,
    pub token_type: String,
}

/// User profile parsed from the id_token JWT
#[derive(Debug, Clone)]
pub struct GoogleProfile {
    /// Stable Google user ID
    pub sub: String,
    /// Email address
    pub email: String,
    /// Full display name
    pub name: String,
    /// Profile photo URL
    pub picture: Option<String>,
    /// Locale (e.g., "en")
    pub locale: Option<String>,
}

/// JWT Claims from Google's id_token
#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    sub: String,
    email: String,
    name: Option<String>,
    picture: Option<String>,
    locale: Option<String>,
    #[serde(rename = "email_verified")]
    _email_verified: Option<bool>,
}

/// Find a free TCP port on localhost for the OAuth redirect server
pub fn find_free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .context("Failed to bind to loopback for port discovery")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// Build the Google OAuth authorization URL
pub fn build_auth_url(pkce: &PkceChallenge, redirect_uri: &str) -> String {
    let scopes = config::OAUTH_SCOPES.join(" ");

    format!(
        "https://accounts.google.com/o/oauth2/v2/auth\
         ?client_id={}\
         &redirect_uri={}\
         &response_type=code\
         &scope={}\
         &code_challenge={}\
         &code_challenge_method=S256\
         &state={}\
         &access_type=offline\
         &prompt=consent",
        urlencoding::encode(config::CLIENT_ID),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(&scopes),
        urlencoding::encode(&pkce.code_challenge),
        urlencoding::encode(&pkce.state),
    )
}

/// Run a local HTTP server to capture the OAuth redirect callback.
///
/// Listens on 127.0.0.1:{port} and waits for Google to redirect back
/// with the authorization code. Sends the code through the oneshot channel.
pub async fn run_redirect_server(
    port: u16,
    tx: oneshot::Sender<AuthCallback>,
) -> Result<()> {
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::{body::Incoming, Request, Response};
    use http_body_util::Full;
    use hyper::body::Bytes;
    use hyper_util::rt::TokioIo;

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    debug!("OAuth redirect server listening on 127.0.0.1:{}", port);

    // We only need to handle one request (the redirect), then shut down
    let tx = std::sync::Arc::new(tokio::sync::Mutex::new(Some(tx)));

    let (stream, _) = listener.accept().await?;
    let io = TokioIo::new(stream);
    let tx_clone = tx.clone();

    let service = service_fn(move |req: Request<Incoming>| {
        let tx = tx_clone.clone();
        async move {
            // Parse query parameters from the redirect URL
            let query = req.uri().query().unwrap_or("");
            let params: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();

            if let (Some(code), Some(state)) = (params.get("code"), params.get("state")) {
                // Send the auth callback through the channel
                if let Some(sender) = tx.lock().await.take() {
                    let _ = sender.send(AuthCallback {
                        code: code.clone(),
                        state: state.clone(),
                    });
                }

                // Return a success page to the browser
                let html = r#"<!DOCTYPE html>
<html>
<head><title>AuraOS</title>
<style>
body { font-family: 'Segoe UI', sans-serif; display: flex; justify-content: center;
       align-items: center; min-height: 100vh; margin: 0; background: #1a1a2e; color: #e0e0e0; }
.container { text-align: center; padding: 2rem; }
h1 { color: #00d4ff; font-size: 2rem; }
p { color: #a0a0b0; font-size: 1.1rem; }
.check { font-size: 4rem; color: #00ff88; }
</style>
</head>
<body>
<div class="container">
  <div class="check">✓</div>
  <h1>Signed in to AuraOS</h1>
  <p>You can close this window and return to AuraOS.</p>
</div>
</body>
</html>"#;

                Ok::<_, hyper::Error>(Response::new(Full::new(Bytes::from(html))))
            } else if let Some(error) = params.get("error") {
                error!("OAuth error from Google: {}", error);

                let html = format!(
                    r#"<!DOCTYPE html><html><body><h1>Sign-in Error</h1><p>{}</p></body></html>"#,
                    error
                );

                Ok(Response::new(Full::new(Bytes::from(html))))
            } else {
                Ok(Response::new(Full::new(Bytes::from("Invalid request"))))
            }
        }
    });

    http1::Builder::new()
        .serve_connection(io, service)
        .await
        .context("Failed to serve OAuth redirect")?;

    Ok(())
}

/// Exchange the authorization code for access/refresh tokens
pub async fn exchange_code(
    auth_code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse> {
    let client = reqwest::Client::new();

    let params = [
        ("client_id", config::CLIENT_ID),
        ("client_secret", config::CLIENT_SECRET),
        ("code", auth_code),
        ("code_verifier", code_verifier),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
    ];

    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&params)
        .send()
        .await
        .context("Failed to send token exchange request")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Token exchange failed (HTTP {}): {}", status, body);
    }

    let tokens: TokenResponse = resp
        .json()
        .await
        .context("Failed to parse token exchange response")?;

    Ok(tokens)
}

/// Parse the id_token JWT to extract user profile information.
///
/// We decode the payload without verifying the signature here because:
/// 1. We received the token directly from Google over TLS
/// 2. The token was exchanged using our client credentials
/// 3. For additional security, production code should verify against Google's JWKS
pub fn parse_id_token(id_token: &str) -> Result<GoogleProfile> {
    // JWT format: header.payload.signature
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() != 3 {
        anyhow::bail!("Invalid id_token format (expected 3 parts, got {})", parts.len());
    }

    // Decode the payload (second part)
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .or_else(|_| {
            // Try with padding
            let padded = match parts[1].len() % 4 {
                2 => format!("{}==", parts[1]),
                3 => format!("{}=", parts[1]),
                _ => parts[1].to_string(),
            };
            base64::engine::general_purpose::URL_SAFE.decode(&padded)
        })
        .context("Failed to base64-decode id_token payload")?;

    let claims: IdTokenClaims = serde_json::from_slice(&payload_bytes)
        .context("Failed to parse id_token claims")?;

    Ok(GoogleProfile {
        sub: claims.sub,
        email: claims.email,
        name: claims.name.unwrap_or_else(|| "AuraOS User".to_string()),
        picture: claims.picture,
        locale: claims.locale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_generation() {
        let pkce = PkceChallenge::generate();

        // Verifier should be base64url-encoded 32 bytes ≈ 43 chars
        assert!(pkce.code_verifier.len() >= 40);
        assert!(pkce.code_challenge.len() >= 40);
        assert!(pkce.state.len() >= 20);

        // Verify the challenge is the SHA256 of the verifier
        let mut hasher = Sha256::new();
        hasher.update(pkce.code_verifier.as_bytes());
        let hash = hasher.finalize();
        let expected_challenge = URL_SAFE_NO_PAD.encode(hash);
        assert_eq!(pkce.code_challenge, expected_challenge);
    }

    #[test]
    fn test_build_auth_url() {
        let pkce = PkceChallenge::generate();
        let url = build_auth_url(&pkce, "http://127.0.0.1:12345");

        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
    }

    #[test]
    fn test_find_free_port() {
        let port = find_free_port().unwrap();
        assert!(port > 0);
    }
}
