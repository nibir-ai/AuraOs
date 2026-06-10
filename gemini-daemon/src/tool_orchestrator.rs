// tool_orchestrator.rs — ReAct Agentic Task Loop
//
// Implements the Reason + Act loop where Gemini is given a task and
// iteratively calls tools until completion. This is the core of the
// agentic framework described in the blueprint.
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use std::fmt;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::cloud_client::AuraCloudClient;
use crate::confirmation::DesktopConfirmation;
use crate::conversation::ConversationManager;
use crate::credentials::CredentialStore;
use crate::tool_registry::ToolRegistry;

/// Maximum number of tool-calling iterations to prevent infinite loops
const MAX_ITERATIONS: usize = 10;

/// Timeout for individual tool executions (30 seconds)
const TOOL_TIMEOUT_SECS: u64 = 30;

/// Status of an agentic task
#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskStatus::Pending => write!(f, "pending"),
            TaskStatus::Running => write!(f, "running"),
            TaskStatus::Completed => write!(f, "completed"),
            TaskStatus::Failed => write!(f, "failed"),
            TaskStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Handle to a running task
#[derive(Debug, Clone)]
pub struct TaskHandle {
    pub task_id: String,
    status: Arc<RwLock<TaskStatus>>,
    result: Arc<RwLock<Option<String>>>,
    tools_used: Arc<RwLock<Vec<String>>>,
    cancelled: Arc<RwLock<bool>>,
}

impl TaskHandle {
    pub fn new(task_id: String) -> Self {
        Self {
            task_id,
            status: Arc::new(RwLock::new(TaskStatus::Pending)),
            result: Arc::new(RwLock::new(None)),
            tools_used: Arc::new(RwLock::new(Vec::new())),
            cancelled: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn is_cancelled(&self) -> bool {
        *self.cancelled.read().await
    }

    pub fn cancel(&self) {
        let cancelled = self.cancelled.clone();
        tokio::spawn(async move {
            *cancelled.write().await = true;
        });
    }

    pub fn set_status(&self, status: TaskStatus) {
        let s = self.status.clone();
        tokio::spawn(async move {
            *s.write().await = status;
        });
    }

    pub fn get_status(&self) -> TaskStatus {
        // Blocking read for D-Bus interface compatibility
        TaskStatus::Running // placeholder for sync context
    }

    pub fn set_result(&self, result: String) {
        let r = self.result.clone();
        tokio::spawn(async move {
            *r.write().await = Some(result);
        });
    }

    pub fn get_result(&self) -> Option<String> {
        None // placeholder for sync context
    }
}

/// Orchestrator that executes agentic tasks using the ReAct loop
pub struct ToolOrchestrator {
    cloud_client: Arc<AuraCloudClient>,
    tool_registry: Arc<ToolRegistry>,
    conv_manager: Arc<ConversationManager>,
    cred_store: Arc<CredentialStore>,
}

impl ToolOrchestrator {
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
        }
    }

    /// Execute a task using the ReAct (Reason + Act) loop
    ///
    /// 1. Send task description + tool definitions to Gemini
    /// 2. If Gemini returns a tool call → execute the tool
    /// 3. Send tool result back to Gemini
    /// 4. Repeat until Gemini returns a final text response (no tool calls)
    pub async fn execute_task(
        &self,
        task_id: &str,
        task_description: &str,
        access_token: &str,
    ) -> Result<String> {
        info!("Starting agentic task (id={}): {}", task_id, task_description);

        let tool_defs = self.tool_registry.get_tool_definitions();
        let mut tools_used: Vec<String> = Vec::new();
        let mut iteration = 0;

        // Add initial user message
        self.conv_manager
            .add_user_message(task_id, task_description)
            .await?;

        loop {
            iteration += 1;

            if iteration > MAX_ITERATIONS {
                warn!("Task {} hit max iterations ({}), forcing completion",
                      task_id, MAX_ITERATIONS);
                return Ok("I've reached the maximum number of steps for this task. \
                           Here's what I accomplished so far.".to_string());
            }

            info!("Task {} — Iteration {}/{}", task_id, iteration, MAX_ITERATIONS);

            // Get current conversation history
            let history = self.conv_manager.get_history(task_id).await?;

            // Send to Gemini via aura-cloud
            let response = self.cloud_client
                .stream_query(
                    access_token,
                    task_id,
                    &history,
                    if iteration == 1 { task_description } else { "" },
                    &tool_defs,
                    "gemini-2.0-flash",
                )
                .await
                .context("aura-cloud query failed")?;

            // Check if Gemini wants to call a tool
            if response.tool_calls.is_empty() {
                // No tool calls — this is the final response
                self.conv_manager
                    .add_assistant_message(task_id, &response.text)
                    .await?;

                info!("Task {} completed after {} iterations (tools: {:?})",
                      task_id, iteration, tools_used);

                return Ok(response.text);
            }

            // Execute each tool call
            for tool_call in &response.tool_calls {
                info!("Task {} — Calling tool: {}({})",
                      task_id, tool_call.tool_name, tool_call.call_id);

                // Record tool call in conversation
                self.conv_manager
                    .add_tool_call(
                        task_id,
                        &tool_call.tool_name,
                        &tool_call.arguments.to_string(),
                        &tool_call.call_id,
                    )
                    .await?;

                // Look up the tool
                let tool = self.tool_registry
                    .get(&tool_call.tool_name)
                    .ok_or_else(|| anyhow::anyhow!("Unknown tool: {}", tool_call.tool_name))?;

                // Check if this tool requires user confirmation
                if tool.requires_confirmation() {
                    let description = format!(
                        "Gemini wants to use '{}' with:\n{}",
                        tool_call.tool_name,
                        serde_json::to_string_pretty(&tool_call.arguments)
                            .unwrap_or_default()
                    );

                    let approved = DesktopConfirmation::show(
                        &format!("Gemini: {}", tool_call.tool_name),
                        &description,
                    )
                    .await;

                    if !approved {
                        warn!("User denied tool call: {}", tool_call.tool_name);

                        self.conv_manager
                            .add_tool_response(
                                task_id,
                                &tool_call.call_id,
                                "User denied this action",
                                false,
                            )
                            .await?;

                        continue;
                    }
                }

                // Execute the tool with a timeout
                let tool_result = tokio::time::timeout(
                    std::time::Duration::from_secs(TOOL_TIMEOUT_SECS),
                    tool.execute(tool_call.arguments.clone(), access_token),
                )
                .await;

                match tool_result {
                    Ok(Ok(result)) => {
                        let result_str = serde_json::to_string(&result.data)
                            .unwrap_or_else(|_| result.data.to_string());

                        // Sanitize output (max 2000 chars per blueprint)
                        let sanitized = if result_str.len() > 2000 {
                            format!("{}... [truncated]", &result_str[..2000])
                        } else {
                            result_str.clone()
                        };

                        self.conv_manager
                            .add_tool_response(
                                task_id,
                                &tool_call.call_id,
                                &sanitized,
                                true,
                            )
                            .await?;

                        tools_used.push(tool_call.tool_name.clone());
                        debug!("Tool {} executed successfully", tool_call.tool_name);
                    }
                    Ok(Err(e)) => {
                        error!("Tool {} failed: {}", tool_call.tool_name, e);

                        self.conv_manager
                            .add_tool_response(
                                task_id,
                                &tool_call.call_id,
                                &format!("Error: {}", e),
                                false,
                            )
                            .await?;
                    }
                    Err(_) => {
                        error!("Tool {} timed out after {}s",
                               tool_call.tool_name, TOOL_TIMEOUT_SECS);

                        self.conv_manager
                            .add_tool_response(
                                task_id,
                                &tool_call.call_id,
                                "Error: tool execution timed out",
                                false,
                            )
                            .await?;
                    }
                }
            }
        }
    }
}
