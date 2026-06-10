// aura-auth-helper — AuraOS OAuth 2.0 PKCE Authentication Helper
//
// This binary orchestrates the first-boot Google sign-in experience (OOBE)
// and can also be invoked to re-authenticate or add additional Google accounts.
//
// Flow:
// 1. Generate PKCE code verifier/challenge
// 2. Launch browser to Google's OAuth consent screen
// 3. Spawn local HTTP server on loopback to capture the redirect
// 4. Exchange authorization code for tokens
// 5. Parse id_token JWT for user profile
// 6. Create Linux user account
// 7. Store tokens in GNOME Keyring
// 8. Write account metadata to /var/lib/auraos/accounts/
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

mod oauth;
mod account;
mod keyring;
mod config;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{info, error, warn};

#[derive(Parser, Debug)]
#[command(
    name = "aura-auth-helper",
    about = "AuraOS Google Account authentication helper",
    version
)]
struct Args {
    /// Run in first-boot (OOBE) mode
    #[arg(long)]
    oobe: bool,

    /// Add an additional Google account (non-OOBE)
    #[arg(long)]
    add_account: bool,

    /// Re-authenticate an existing account (token revoked)
    #[arg(long)]
    reauth: Option<String>,

    /// Set offline PIN for an account
    #[arg(long)]
    set_pin: Option<String>,

    /// Account database path
    #[arg(long, default_value = "/var/lib/auraos/accounts")]
    account_db: String,

    /// Enable debug logging
    #[arg(long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    let filter = if args.debug { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    info!("AuraOS Auth Helper v{}", env!("CARGO_PKG_VERSION"));

    if args.oobe || args.add_account {
        run_oauth_flow(&args).await?;
    } else if let Some(ref username) = args.reauth {
        run_reauth(username, &args).await?;
    } else if let Some(ref username) = args.set_pin {
        run_set_pin(username, &args).await?;
    } else {
        // Default: run OOBE mode
        run_oauth_flow(&args).await?;
    }

    Ok(())
}

/// Run the full OAuth PKCE flow: browser sign-in → token exchange → account creation
async fn run_oauth_flow(args: &Args) -> Result<()> {
    info!("Starting Google OAuth sign-in flow...");

    // Step 1: Generate PKCE parameters
    let pkce = oauth::PkceChallenge::generate();
    info!("PKCE challenge generated");

    // Step 2: Find a free port for the redirect server
    let port = oauth::find_free_port().context("Failed to find free port for OAuth redirect")?;
    let redirect_uri = format!("http://127.0.0.1:{}", port);

    // Step 3: Build the authorization URL
    let auth_url = oauth::build_auth_url(&pkce, &redirect_uri);
    info!("Authorization URL constructed");

    // Step 4: Start the local redirect server (in background)
    let (auth_code_tx, auth_code_rx) = tokio::sync::oneshot::channel::<oauth::AuthCallback>();

    let server_handle = tokio::spawn(async move {
        oauth::run_redirect_server(port, auth_code_tx).await
    });

    // Step 5: Open the browser for user authentication
    info!("Opening browser for Google sign-in...");
    if let Err(e) = open_browser(&auth_url) {
        warn!("Failed to open browser automatically: {}", e);
        println!("\n╔══════════════════════════════════════════════════════════════╗");
        println!("║  Please open this URL in your browser to sign in:          ║");
        println!("╚══════════════════════════════════════════════════════════════╝");
        println!("\n{}\n", auth_url);
    }

    // Step 6: Wait for the callback with the authorization code
    info!("Waiting for Google sign-in callback...");
    let callback = auth_code_rx.await
        .context("Redirect server shut down without receiving callback")?;

    // Verify state parameter (CSRF protection)
    if callback.state != pkce.state {
        error!("State parameter mismatch — possible CSRF attack");
        anyhow::bail!("OAuth state mismatch");
    }

    let auth_code = callback.code;
    info!("Authorization code received");

    // Step 7: Exchange the authorization code for tokens
    info!("Exchanging authorization code for tokens...");
    let tokens = oauth::exchange_code(&auth_code, &pkce.code_verifier, &redirect_uri)
        .await
        .context("Token exchange failed")?;

    info!("Tokens received (access_token expires in {}s)", tokens.expires_in);

    // Step 8: Parse the id_token for user profile information
    let profile = oauth::parse_id_token(&tokens.id_token)
        .context("Failed to parse id_token")?;

    info!("User profile: {} ({})", profile.name, profile.email);

    // Step 9: Create the Linux user account
    info!("Creating Linux user account...");
    let linux_username = account::create_user_account(&profile, &args.account_db)
        .context("Failed to create Linux user account")?;

    info!("Linux user '{}' created", linux_username);

    // Step 10: Store tokens in GNOME Keyring
    info!("Storing tokens in GNOME Keyring...");
    keyring::store_refresh_token(&profile.sub, &tokens.refresh_token)
        .await
        .context("Failed to store refresh token")?;

    keyring::cache_access_token(&profile.sub, &tokens.access_token, tokens.expires_in)
        .await
        .context("Failed to cache access token")?;

    info!("Tokens stored securely");

    // Step 11: Write account metadata
    info!("Writing account metadata...");
    account::write_account_metadata(&profile, &linux_username, &tokens, &args.account_db)
        .context("Failed to write account metadata")?;

    // Step 12: Download avatar
    if let Some(ref picture_url) = profile.picture {
        info!("Downloading profile avatar...");
        if let Err(e) = account::download_avatar(picture_url, &linux_username).await {
            warn!("Failed to download avatar: {} (non-fatal)", e);
        }
    }

    // Shut down the redirect server
    server_handle.abort();

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║  ✓ AuraOS account setup complete!                          ║");
    println!("║                                                            ║");
    println!("║  Name:     {:<47}║", profile.name);
    println!("║  Email:    {:<47}║", profile.email);
    println!("║  Username: {:<47}║", linux_username);
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    info!("Account setup complete for {} ({})", profile.name, profile.email);

    Ok(())
}

/// Re-authenticate an existing account (e.g., after token revocation)
async fn run_reauth(username: &str, args: &Args) -> Result<()> {
    info!("Re-authenticating user '{}'...", username);

    // Look up existing account
    let account = account::lookup_account_by_username(username, &args.account_db)
        .context("Account not found")?;

    // Run OAuth flow (same as initial, but skip user creation)
    let pkce = oauth::PkceChallenge::generate();
    let port = oauth::find_free_port()?;
    let redirect_uri = format!("http://127.0.0.1:{}", port);
    let auth_url = oauth::build_auth_url(&pkce, &redirect_uri);

    let (auth_code_tx, auth_code_rx) = tokio::sync::oneshot::channel();
    let server_handle = tokio::spawn(async move {
        oauth::run_redirect_server(port, auth_code_tx).await
    });

    let _ = open_browser(&auth_url);
    println!("\nPlease sign in with your Google account to re-authenticate.\n{}\n", auth_url);

    let callback = auth_code_rx.await?;
    if callback.state != pkce.state {
        anyhow::bail!("OAuth state mismatch");
    }

    let tokens = oauth::exchange_code(&callback.code, &pkce.code_verifier, &redirect_uri).await?;

    // Update stored tokens
    keyring::store_refresh_token(&account.sub, &tokens.refresh_token).await?;
    keyring::cache_access_token(&account.sub, &tokens.access_token, tokens.expires_in).await?;

    server_handle.abort();

    info!("Re-authentication complete for '{}'", username);
    println!("✓ Account re-authenticated successfully.");

    Ok(())
}

/// Set an offline PIN for an existing account
async fn run_set_pin(username: &str, args: &Args) -> Result<()> {
    info!("Setting offline PIN for user '{}'", username);

    let _account = account::lookup_account_by_username(username, &args.account_db)
        .context("Account not found")?;

    // Prompt for PIN (read from stdin securely)
    let pin = rpassword::prompt_password("Enter new offline PIN (6-32 chars): ")
        .context("Failed to read PIN")?;

    let pin_confirm = rpassword::prompt_password("Confirm offline PIN: ")
        .context("Failed to read PIN confirmation")?;

    if pin != pin_confirm {
        anyhow::bail!("PINs do not match");
    }

    if pin.len() < 6 || pin.len() > 32 {
        anyhow::bail!("PIN must be 6-32 characters");
    }

    // Hash and store the PIN
    account::set_offline_pin(username, &pin, &args.account_db)
        .context("Failed to set offline PIN")?;

    info!("Offline PIN set for '{}'", username);
    println!("✓ Offline PIN configured successfully.");

    Ok(())
}

/// Open a URL in the system browser
fn open_browser(url: &str) -> Result<()> {
    std::process::Command::new("xdg-open")
        .arg(url)
        .spawn()
        .context("Failed to launch browser via xdg-open")?;
    Ok(())
}
