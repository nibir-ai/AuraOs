// gemini-daemon — AuraOS Gemini Personal Assistant System Service
//
// Long-running privileged service that acts as the single broker between
// Gemini's API and all system components. Applications communicate via
// the com.auraos.GeminiAssistant D-Bus interface.
//
// Architecture:
// - D-Bus server on session bus (com.auraos.GeminiAssistant)
// - gRPC client to aura-cloud backend (Gemini API proxy)
// - Tool registry with Google API tool implementations
// - ReAct agentic loop for multi-step task execution
// - SQLite-backed conversation persistence
// - ProactiveScheduler for morning briefings, follow-up detection
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

mod dbus_interface;
mod conversation;
mod cloud_client;
mod tool_registry;
mod tool_orchestrator;
mod proactive;
mod confirmation;
mod credentials;
mod tools;

use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::{info, error};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("AURA_LOG").unwrap_or_else(|_| "info".to_string())
        )
        .with_target(true)
        .init();

    info!("gemini-daemon v{} starting...", env!("CARGO_PKG_VERSION"));

    // Step 1: Load configuration
    let config = load_config()?;
    info!("Configuration loaded from {}", config.config_path);

    // Step 2: Initialize credential store (connect to GNOME Keyring)
    let cred_store = Arc::new(
        credentials::CredentialStore::connect()
            .await
            .context("Failed to connect to credential store")?
    );
    info!("Credential store connected");

    // Step 3: Initialize tool registry
    let tool_registry = Arc::new(
        tool_registry::ToolRegistry::new()
            .register(Box::new(tools::gmail_search::GmailSearchTool::new(cred_store.clone())))
            .register(Box::new(tools::gmail_send::GmailSendTool::new(cred_store.clone())))
            .register(Box::new(tools::gmail_draft::GmailDraftTool::new(cred_store.clone())))
            .register(Box::new(tools::calendar_query::CalendarQueryTool::new(cred_store.clone())))
            .register(Box::new(tools::calendar_create::CalendarCreateTool::new(cred_store.clone())))
            .register(Box::new(tools::calendar_delete::CalendarDeleteTool::new(cred_store.clone())))
            .register(Box::new(tools::drive_search::DriveSearchTool::new(cred_store.clone())))
            .register(Box::new(tools::drive_read::DriveReadTool::new(cred_store.clone())))
            .register(Box::new(tools::tasks_query::TasksQueryTool::new(cred_store.clone())))
            .register(Box::new(tools::tasks_complete::TasksCompleteTool::new(cred_store.clone())))
            .register(Box::new(tools::contacts_search::ContactsSearchTool::new(cred_store.clone())))
            .register(Box::new(tools::local_file_read::LocalFileReadTool::new()))
            .register(Box::new(tools::system_notification::SystemNotificationTool::new()))
            .build()
    );
    info!("Tool registry initialized ({} tools)", tool_registry.tool_count());

    // Step 4: Initialize aura-cloud gRPC client
    let cloud_client = Arc::new(
        cloud_client::AuraCloudClient::connect(&config.cloud_url)
            .await
            .context("Failed to connect to aura-cloud backend")?
    );
    info!("aura-cloud client connected to {}", config.cloud_url);

    // Step 5: Initialize conversation manager (SQLite persistence)
    let conv_manager = Arc::new(
        conversation::ConversationManager::new(&config.conversation_db)
            .await
            .context("Failed to initialize conversation manager")?
    );
    info!("Conversation manager initialized");

    // Step 6: Initialize and spawn the proactive scheduler
    let scheduler = proactive::ProactiveScheduler::new(
        tool_registry.clone(),
        cloud_client.clone(),
        cred_store.clone(),
    );
    let scheduler_handle = tokio::spawn(async move {
        if let Err(e) = scheduler.run().await {
            error!("ProactiveScheduler error: {}", e);
        }
    });
    info!("ProactiveScheduler started");

    // Step 7: Expose D-Bus service
    let daemon = dbus_interface::GeminiDaemon::new(
        cloud_client,
        tool_registry,
        conv_manager,
        cred_store,
    );

    let _conn = zbus::connection::Builder::session()
        .context("Failed to create D-Bus session builder")?
        .name("com.auraos.GeminiAssistant")
        .context("Failed to acquire D-Bus name")?
        .serve_at("/com/auraos/GeminiAssistant", daemon)
        .context("Failed to serve D-Bus interface")?
        .build()
        .await
        .context("Failed to build D-Bus connection")?;

    info!("D-Bus service registered: com.auraos.GeminiAssistant");
    info!("gemini-daemon is ready and listening for requests");

    // Keep the daemon alive until terminated
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Received SIGINT, shutting down...");
        }
        _ = async {
            // Wait for SIGTERM
            #[cfg(unix)]
            {
                let mut sig = tokio::signal::unix::signal(
                    tokio::signal::unix::SignalKind::terminate()
                ).unwrap();
                sig.recv().await;
            }
            #[cfg(not(unix))]
            {
                std::future::pending::<()>().await;
            }
        } => {
            info!("Received SIGTERM, shutting down...");
        }
    }

    // Cleanup
    scheduler_handle.abort();
    info!("gemini-daemon shut down cleanly");

    Ok(())
}

/// Daemon configuration
struct DaemonConfig {
    config_path: String,
    cloud_url: String,
    conversation_db: String,
}

fn load_config() -> Result<DaemonConfig> {
    let config_path = std::env::var("AURA_CONFIG")
        .unwrap_or_else(|_| "/etc/auraos/config.toml".to_string());

    // Try to read the config file
    let cloud_url;
    let conversation_db;

    if let Ok(content) = std::fs::read_to_string(&config_path) {
        let config: toml::Value = content.parse()
            .context("Failed to parse config.toml")?;

        cloud_url = config
            .get("cloud")
            .and_then(|c| c.get("url"))
            .and_then(|u| u.as_str())
            .unwrap_or("https://api.auraos.io:443")
            .to_string();

        conversation_db = config
            .get("gemini")
            .and_then(|g| g.get("conversation_db"))
            .and_then(|d| d.as_str())
            .unwrap_or("/var/lib/auraos/gemini-conversations.db")
            .to_string();
    } else {
        // Use defaults if config file doesn't exist
        cloud_url = "https://api.auraos.io:443".to_string();
        conversation_db = "/var/lib/auraos/gemini-conversations.db".to_string();
    }

    Ok(DaemonConfig {
        config_path,
        cloud_url,
        conversation_db,
    })
}
