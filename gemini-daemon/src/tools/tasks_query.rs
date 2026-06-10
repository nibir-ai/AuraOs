// tools/tasks_query.rs — Google Tasks Query Tool
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use crate::credentials::CredentialStore;
use super::{GeminiTool, ToolResult, google_api_get};

pub struct TasksQueryTool { _cred_store: Arc<CredentialStore> }
impl TasksQueryTool {
    pub fn new(cred_store: Arc<CredentialStore>) -> Self { Self { _cred_store: cred_store } }
}

#[async_trait]
impl GeminiTool for TasksQueryTool {
    fn name(&self) -> &'static str { "tasks_query" }
    fn description(&self) -> &'static str { "Query the user's Google Tasks lists and tasks." }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "list_id": { "type": "string", "description": "Task list ID (default: '@default')" },
                "show_completed": { "type": "boolean", "default": false },
                "max_results": { "type": "integer", "default": 20 }
            }
        })
    }

    async fn execute(&self, input: Value, access_token: &str) -> Result<ToolResult, anyhow::Error> {
        let list_id = input["list_id"].as_str().unwrap_or("@default");
        let max = input["max_results"].as_u64().unwrap_or(20);
        let show_completed = input["show_completed"].as_bool().unwrap_or(false);

        let url = format!(
            "https://tasks.googleapis.com/tasks/v1/lists/{}/tasks?maxResults={}&showCompleted={}",
            urlencoding::encode(list_id), max, show_completed
        );

        let result = google_api_get(&url, access_token).await?;
        let tasks: Vec<Value> = result["items"].as_array().cloned().unwrap_or_default()
            .into_iter()
            .map(|t| json!({
                "id": t["id"], "title": t["title"], "status": t["status"],
                "due": t["due"], "notes": t["notes"], "updated": t["updated"]
            }))
            .collect();

        Ok(ToolResult::success(json!({ "tasks": tasks, "count": tasks.len() })))
    }
}
