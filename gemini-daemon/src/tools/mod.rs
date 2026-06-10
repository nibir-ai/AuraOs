// tools/mod.rs — Gemini Tool Trait and Module Declarations
//
// Defines the GeminiTool trait that all tools must implement, and
// re-exports all tool implementations.
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use serde_json::Value;

pub mod gmail_search;
pub mod gmail_send;
pub mod gmail_draft;
pub mod calendar_query;
pub mod calendar_create;
pub mod calendar_delete;
pub mod drive_search;
pub mod drive_read;
pub mod tasks_query;
pub mod tasks_complete;
pub mod contacts_search;
pub mod local_file_read;
pub mod system_notification;

/// Result of a tool execution
#[derive(Debug)]
pub struct ToolResult {
    pub data: Value,
    pub success: bool,
    pub error: Option<String>,
}

impl ToolResult {
    pub fn success(data: Value) -> Self {
        Self {
            data,
            success: true,
            error: None,
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        let msg = msg.into();
        Self {
            data: serde_json::json!({ "error": msg }),
            success: false,
            error: Some(msg),
        }
    }
}

/// Trait that all Gemini tools must implement
///
/// Each tool represents a capability that Gemini can invoke via
/// function calling. Tools are registered in the ToolRegistry and
/// dispatched by the ToolOrchestrator.
#[async_trait]
pub trait GeminiTool: Send + Sync {
    /// Unique tool name (matches Gemini function calling name)
    fn name(&self) -> &'static str;

    /// Human-readable description for Gemini
    fn description(&self) -> &'static str;

    /// JSON Schema defining the tool's input parameters
    fn input_schema(&self) -> Value;

    /// Execute the tool with the given input and user's access token
    async fn execute(&self, input: Value, access_token: &str) -> Result<ToolResult, anyhow::Error>;

    /// Whether this tool requires user confirmation before execution
    /// (true for destructive actions: send email, create event, etc.)
    fn requires_confirmation(&self) -> bool {
        false
    }
}

/// Helper to make authenticated Google API requests
pub async fn google_api_get(url: &str, access_token: &str) -> Result<Value, anyhow::Error> {
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Google API error (HTTP {}): {}", status, body);
    }

    let json: Value = resp.json().await?;
    Ok(json)
}

/// Helper to make authenticated Google API POST requests
pub async fn google_api_post(
    url: &str,
    access_token: &str,
    body: &Value,
) -> Result<Value, anyhow::Error> {
    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {}", access_token))
        .json(body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Google API error (HTTP {}): {}", status, body);
    }

    let json: Value = resp.json().await?;
    Ok(json)
}

/// Helper to make authenticated Google API DELETE requests
pub async fn google_api_delete(url: &str, access_token: &str) -> Result<(), anyhow::Error> {
    let client = reqwest::Client::new();
    let resp = client
        .delete(url)
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Google API DELETE error (HTTP {}): {}", status, body);
    }

    Ok(())
}
