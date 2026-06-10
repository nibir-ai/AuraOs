// cloud_client.rs — aura-cloud gRPC Client
//
// Connects to the aura-cloud backend to proxy Gemini API requests.
// The backend holds the Gemini API key — this client never sees it.
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use serde_json;
use tracing::{debug, info};

use crate::conversation::Message;

/// Client for the aura-cloud gRPC backend service
pub struct AuraCloudClient {
    endpoint: String,
    http_client: reqwest::Client,
}

impl AuraCloudClient {
    /// Connect to the aura-cloud backend
    pub async fn connect(endpoint: &str) -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .context("Failed to build HTTP client")?;

        info!("AuraCloudClient connecting to {}", endpoint);

        Ok(Self {
            endpoint: endpoint.to_string(),
            http_client,
        })
    }

    /// Send a synchronous query to Gemini via aura-cloud
    pub async fn query(
        &self,
        access_token: &str,
        conversation_id: &str,
        history: &[Message],
        user_message: &str,
        tools: &[ToolDef],
        model: &str,
        max_tokens: u32,
    ) -> Result<String> {
        let request = QueryRequest {
            google_access_token: access_token.to_string(),
            conversation_id: conversation_id.to_string(),
            history: history.to_vec(),
            user_message: user_message.to_string(),
            tools: tools.to_vec(),
            model: model.to_string(),
            max_tokens,
            system_context: SystemContext::current(),
        };

        let url = format!("{}/v1/query", self.endpoint);

        debug!("Sending query to aura-cloud: {}", url);

        let resp = self.http_client
            .post(&url)
            .header("x-google-access-token", access_token)
            .json(&request)
            .send()
            .await
            .context("Failed to reach aura-cloud")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("aura-cloud returned HTTP {}: {}", status, body);
        }

        let response: QueryResponse = resp
            .json()
            .await
            .context("Failed to parse aura-cloud response")?;

        Ok(response.text)
    }

    /// Send a streaming query for agentic tasks
    pub async fn stream_query(
        &self,
        access_token: &str,
        conversation_id: &str,
        history: &[Message],
        user_message: &str,
        tools: &[ToolDef],
        model: &str,
    ) -> Result<StreamingResponse> {
        let request = QueryRequest {
            google_access_token: access_token.to_string(),
            conversation_id: conversation_id.to_string(),
            history: history.to_vec(),
            user_message: user_message.to_string(),
            tools: tools.to_vec(),
            model: model.to_string(),
            max_tokens: 4096,
            system_context: SystemContext::current(),
        };

        let url = format!("{}/v1/stream", self.endpoint);

        let resp = self.http_client
            .post(&url)
            .header("x-google-access-token", access_token)
            .json(&request)
            .send()
            .await
            .context("Failed to reach aura-cloud for streaming")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("aura-cloud stream returned HTTP {}: {}", status, body);
        }

        let response: StreamingResponse = resp
            .json()
            .await
            .context("Failed to parse streaming response")?;

        Ok(response)
    }
}

// ─── Request/Response Types ──────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
struct QueryRequest {
    google_access_token: String,
    conversation_id: String,
    history: Vec<Message>,
    user_message: String,
    tools: Vec<ToolDef>,
    model: String,
    max_tokens: u32,
    system_context: SystemContext,
}

#[derive(Debug, serde::Deserialize)]
struct QueryResponse {
    text: String,
    #[serde(default)]
    tool_calls: Vec<ToolCallResponse>,
    #[serde(default)]
    usage: Option<UsageMetrics>,
}

#[derive(Debug, serde::Deserialize)]
pub struct StreamingResponse {
    pub text: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallResponse>,
    pub is_final: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, serde::Deserialize)]
pub struct ToolCallResponse {
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub call_id: String,
}

#[derive(Debug, serde::Serialize)]
struct SystemContext {
    distro_version: String,
    active_app: String,
    locale: String,
    timezone: String,
    current_datetime: String,
}

#[derive(Debug, serde::Deserialize)]
struct UsageMetrics {
    #[allow(dead_code)]
    input_tokens: u32,
    #[allow(dead_code)]
    output_tokens: u32,
    #[allow(dead_code)]
    total_tokens: u32,
}

impl SystemContext {
    fn current() -> Self {
        Self {
            distro_version: env!("CARGO_PKG_VERSION").to_string(),
            active_app: std::env::var("AURA_ACTIVE_APP").unwrap_or_default(),
            locale: std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".to_string()),
            timezone: std::env::var("TZ").unwrap_or_else(|_| "UTC".to_string()),
            current_datetime: chrono::Utc::now().to_rfc3339(),
        }
    }
}
