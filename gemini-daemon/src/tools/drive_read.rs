// tools/drive_read.rs — Google Drive File Read Tool
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use crate::credentials::CredentialStore;
use super::{GeminiTool, ToolResult, google_api_get};

pub struct DriveReadTool { _cred_store: Arc<CredentialStore> }
impl DriveReadTool {
    pub fn new(cred_store: Arc<CredentialStore>) -> Self { Self { _cred_store: cred_store } }
}

#[async_trait]
impl GeminiTool for DriveReadTool {
    fn name(&self) -> &'static str { "drive_read_file" }
    fn description(&self) -> &'static str {
        "Read the content of a file from Google Drive by its file ID. Works for text-based files."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_id": { "type": "string", "description": "The Google Drive file ID" },
                "export_as": { "type": "string", "description": "Export format for Google Docs (e.g., 'text/plain', 'application/pdf')" }
            },
            "required": ["file_id"]
        })
    }

    async fn execute(&self, input: Value, access_token: &str) -> Result<ToolResult, anyhow::Error> {
        let file_id = input["file_id"].as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing file_id"))?;

        // First get file metadata
        let meta_url = format!(
            "https://www.googleapis.com/drive/v3/files/{}?fields=name,mimeType,size",
            urlencoding::encode(file_id)
        );
        let metadata = google_api_get(&meta_url, access_token).await?;
        let mime_type = metadata["mimeType"].as_str().unwrap_or("");

        // For Google Docs/Sheets/Slides, use export
        let content = if mime_type.starts_with("application/vnd.google-apps") {
            let export_mime = input["export_as"].as_str().unwrap_or("text/plain");
            let export_url = format!(
                "https://www.googleapis.com/drive/v3/files/{}/export?mimeType={}",
                urlencoding::encode(file_id), urlencoding::encode(export_mime)
            );

            let client = reqwest::Client::new();
            let resp = client.get(&export_url)
                .header("Authorization", format!("Bearer {}", access_token))
                .send().await?;

            if !resp.status().is_success() {
                anyhow::bail!("Drive export failed: HTTP {}", resp.status());
            }

            let text = resp.text().await?;
            // Truncate to 2000 chars per blueprint
            if text.len() > 2000 { format!("{}... [truncated]", &text[..2000]) } else { text }
        } else {
            // For regular files, download content
            let dl_url = format!(
                "https://www.googleapis.com/drive/v3/files/{}?alt=media",
                urlencoding::encode(file_id)
            );

            let client = reqwest::Client::new();
            let resp = client.get(&dl_url)
                .header("Authorization", format!("Bearer {}", access_token))
                .send().await?;

            let text = resp.text().await?;
            if text.len() > 2000 { format!("{}... [truncated]", &text[..2000]) } else { text }
        };

        Ok(ToolResult::success(json!({
            "file_name": metadata["name"],
            "mime_type": mime_type,
            "content": content
        })))
    }
}
