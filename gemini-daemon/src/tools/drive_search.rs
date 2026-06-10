// tools/drive_search.rs — Google Drive Search Tool
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use crate::credentials::CredentialStore;
use super::{GeminiTool, ToolResult, google_api_get};

pub struct DriveSearchTool { _cred_store: Arc<CredentialStore> }
impl DriveSearchTool {
    pub fn new(cred_store: Arc<CredentialStore>) -> Self { Self { _cred_store: cred_store } }
}

#[async_trait]
impl GeminiTool for DriveSearchTool {
    fn name(&self) -> &'static str { "drive_search" }
    fn description(&self) -> &'static str {
        "Search for files in the user's Google Drive. Returns file names, types, and IDs."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query (file name or content)" },
                "mime_type": { "type": "string", "description": "Filter by MIME type (e.g., 'application/pdf')" },
                "max_results": { "type": "integer", "default": 10, "maximum": 50 }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value, access_token: &str) -> Result<ToolResult, anyhow::Error> {
        let query = input["query"].as_str().unwrap_or("");
        let max = input["max_results"].as_u64().unwrap_or(10);

        let mut drive_query = format!("name contains '{}'", query.replace('\'', "\\'"));
        if let Some(mime) = input["mime_type"].as_str() {
            drive_query.push_str(&format!(" and mimeType='{}'", mime));
        }

        let url = format!(
            "https://www.googleapis.com/drive/v3/files?q={}&pageSize={}&fields=files(id,name,mimeType,size,modifiedTime,webViewLink)",
            urlencoding::encode(&drive_query), max
        );

        let result = google_api_get(&url, access_token).await?;
        let files: Vec<Value> = result["files"].as_array().cloned().unwrap_or_default()
            .into_iter()
            .map(|f| json!({
                "id": f["id"],
                "name": f["name"],
                "type": f["mimeType"],
                "size": f["size"],
                "modified": f["modifiedTime"],
                "link": f["webViewLink"]
            }))
            .collect();

        Ok(ToolResult::success(json!({ "files": files, "count": files.len() })))
    }
}
