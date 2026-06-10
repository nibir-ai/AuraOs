// dbus_interface.rs — D-Bus Interface for Gemini Daemon
//
// Implements the com.auraos.GeminiAssistant1 D-Bus interface as specified
// in the AuraOS blueprint. This is the primary API surface for all desktop
// applications to interact with Gemini.
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;
use zbus::interface;
use zbus::zvariant::Value;
use tracing::{info, error, debug};

use crate::cloud_client::AuraCloudClient;
use crate::conversation::ConversationManager;
use crate::credentials::CredentialStore;
use crate::tool_orchestrator::{TaskHandle, TaskStatus};
use crate::tool_registry::ToolRegistry;

/// Main D-Bus service object
pub struct GeminiDaemon {
    cloud_client: Arc<AuraCloudClient>,
    tool_registry: Arc<ToolRegistry>,
    conv_manager: Arc<ConversationManager>,
    cred_store: Arc<CredentialStore>,

    /// Active tasks (task_id → handle)
    active_tasks: Arc<RwLock<HashMap<String, TaskHandle>>>,
}

impl GeminiDaemon {
    pub fn new(
        cloud_client: Arc<AuraCloudClient>,
        tool_registry: Arc<ToolRegistry>,
        conv_manager: Arc<ConversationManager>,
        cred_store: Arc<CredentialStore>,
    ) -> Self {
        Self {
            cloud_client,
            tool_registry,
            conv_manager,
            cred_store,
            active_tasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

/// D-Bus interface implementation matching com.auraos.GeminiAssistant1
#[interface(name = "com.auraos.GeminiAssistant1")]
impl GeminiDaemon {
    /// Synchronous single-turn query (short timeout, no tools)
    ///
    /// Used for simple questions that don't require tool execution.
    /// Returns the response text and a task ID for tracking.
    async fn query(
        &self,
        prompt: &str,
        options: HashMap<String, Value<'_>>,
    ) -> zbus::fdo::Result<(String, String)> {
        let task_id = Uuid::new_v4().to_string();
        info!("Query received (task_id={}): '{}'", task_id, truncate(prompt, 80));

        // Get the user's access token
        let access_token = self.cred_store
            .get_current_access_token()
            .await
            .map_err(|e| {
                error!("Failed to get access token: {}", e);
                zbus::fdo::Error::Failed(format!("Authentication error: {}", e))
            })?;

        // Extract options
        let model = options
            .get("model")
            .and_then(|v| v.downcast_ref::<str>())
            .unwrap_or("gemini-2.0-flash");

        let max_tokens = options
            .get("max_tokens")
            .and_then(|v| {
                if let Value::U32(n) = v { Some(*n as u32) } else { None }
            })
            .unwrap_or(2048);

        // Add to conversation history
        self.conv_manager
            .add_user_message(&task_id, prompt)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;

        // Get conversation history for context
        let history = self.conv_manager
            .get_history(&task_id)
            .await
            .unwrap_or_default();

        // Send query to aura-cloud (no tools for sync query)
        let response = self.cloud_client
            .query(
                &access_token,
                &task_id,
                &history,
                prompt,
                &[], // No tools for sync query
                model,
                max_tokens,
            )
            .await
            .map_err(|e| {
                error!("Cloud query failed: {}", e);
                zbus::fdo::Error::Failed(format!("Query failed: {}", e))
            })?;

        // Store response in conversation history
        self.conv_manager
            .add_assistant_message(&task_id, &response)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;

        info!("Query completed (task_id={})", task_id);
        Ok((response, task_id))
    }

    /// Dispatch an async agentic task (long-running, uses tools)
    ///
    /// The task runs in the background using the ReAct loop.
    /// Progress is reported via the StreamChunk and TaskCompleted signals.
    async fn dispatch_task(
        &self,
        task_description: &str,
        context: &str,
        #[zbus(signal_emitter)] emitter: dbus_interface::StreamChunkChanged,
    ) -> zbus::fdo::Result<String> {
        let task_id = Uuid::new_v4().to_string();
        info!("Task dispatched (task_id={}): '{}'", task_id, truncate(task_description, 80));

        // Parse context JSON
        let _context: serde_json::Value = serde_json::from_str(context)
            .unwrap_or(serde_json::json!({}));

        // Get access token
        let access_token = self.cred_store
            .get_current_access_token()
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;

        // Create task handle
        let handle = TaskHandle::new(task_id.clone());

        // Store task handle
        {
            let mut tasks = self.active_tasks.write().await;
            tasks.insert(task_id.clone(), handle.clone());
        }

        // Spawn the agentic task in the background
        let cloud_client = self.cloud_client.clone();
        let tool_registry = self.tool_registry.clone();
        let conv_manager = self.conv_manager.clone();
        let cred_store = self.cred_store.clone();
        let active_tasks = self.active_tasks.clone();
        let task_desc = task_description.to_string();
        let tid = task_id.clone();

        tokio::spawn(async move {
            let orchestrator = crate::tool_orchestrator::ToolOrchestrator::new(
                cloud_client,
                tool_registry,
                conv_manager,
                cred_store,
            );

            let result = orchestrator
                .execute_task(&tid, &task_desc, &access_token)
                .await;

            // Update task status
            let mut tasks = active_tasks.write().await;
            if let Some(handle) = tasks.get_mut(&tid) {
                match result {
                    Ok(response) => {
                        handle.set_status(TaskStatus::Completed);
                        handle.set_result(response);
                        info!("Task completed (task_id={})", tid);
                    }
                    Err(e) => {
                        handle.set_status(TaskStatus::Failed);
                        handle.set_result(format!("Error: {}", e));
                        error!("Task failed (task_id={}): {}", tid, e);
                    }
                }
            }
        });

        Ok(task_id)
    }

    /// Cancel a running task
    async fn cancel_task(&self, task_id: &str) -> zbus::fdo::Result<bool> {
        let mut tasks = self.active_tasks.write().await;
        if let Some(handle) = tasks.get_mut(task_id) {
            handle.cancel();
            info!("Task cancelled (task_id={})", task_id);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get the status of a task
    async fn get_task_status(&self, task_id: &str) -> zbus::fdo::Result<(String, String)> {
        let tasks = self.active_tasks.read().await;
        if let Some(handle) = tasks.get(task_id) {
            let status = handle.get_status().to_string();
            let result = handle.get_result().unwrap_or_default();
            Ok((status, result))
        } else {
            Ok(("unknown".to_string(), String::new()))
        }
    }

    /// Get the conversation history as a JSON array
    async fn get_conversation_history(&self) -> zbus::fdo::Result<String> {
        let history = self.conv_manager
            .get_all_history()
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;

        let json = serde_json::to_string(&history)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;

        Ok(json)
    }

    /// Clear the conversation history
    async fn clear_conversation(&self) -> zbus::fdo::Result<()> {
        self.conv_manager
            .clear_all()
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;

        info!("Conversation history cleared");
        Ok(())
    }

    // ─── Signals ──────────────────────────────────────────────────

    /// Emitted when a streaming response chunk is available
    #[zbus(signal)]
    async fn stream_chunk(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        task_id: &str,
        chunk: &str,
        is_final: bool,
    ) -> zbus::Result<()>;

    /// Emitted when a task completes
    #[zbus(signal)]
    async fn task_completed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        task_id: &str,
        result: &str,
        tools_used: &[String],
    ) -> zbus::Result<()>;

    /// Emitted for proactive insights (morning briefing, meeting alerts, etc.)
    #[zbus(signal)]
    async fn proactive_insight(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        insight_type: &str,
        content: &str,
        actions: &str,
    ) -> zbus::Result<()>;
}

/// Truncate a string for logging purposes
fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..max_len]
    }
}
