// tools/tasks_complete.rs — Google Tasks Complete Tool (destructive)
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use crate::credentials::CredentialStore;
use super::{GeminiTool, ToolResult};

pub struct TasksCompleteTool { _cred_store: Arc<CredentialStore> }
impl TasksCompleteTool {
    pub fn new(cred_store: Arc<CredentialStore>) -> Self { Self { _cred_store: cred_store } }
}

#[async_trait]
impl GeminiTool for TasksCompleteTool {
    fn name(&self) -> &'static str { "tasks_complete" }
    fn description(&self) -> &'static str { "Mark a task as completed in Google Tasks." }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "The task ID" },
                "list_id": { "type": "string", "description": "Task list ID (default: '@default')" }
            },
            "required": ["task_id"]
        })
    }
    fn requires_confirmation(&self) -> bool { true }

    async fn execute(&self, input: Value, access_token: &str) -> Result<ToolResult, anyhow::Error> {
        let task_id = input["task_id"].as_str().ok_or_else(|| anyhow::anyhow!("Missing task_id"))?;
        let list_id = input["list_id"].as_str().unwrap_or("@default");

        let url = format!(
            "https://tasks.googleapis.com/tasks/v1/lists/{}/tasks/{}",
            urlencoding::encode(list_id), urlencoding::encode(task_id)
        );

        let body = json!({ "status": "completed" });
        let client = reqwest::Client::new();
        let resp = client.patch(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .json(&body)
            .send().await?;

        if !resp.status().is_success() {
            anyhow::bail!("Tasks API error: HTTP {}", resp.status());
        }

        Ok(ToolResult::success(json!({ "completed": true, "task_id": task_id })))
    }
}
