// tools/gmail_draft.rs — Gmail Draft Tool
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use crate::credentials::CredentialStore;
use super::{GeminiTool, ToolResult, google_api_post};
use base64::{engine::general_purpose::URL_SAFE, Engine};

pub struct GmailDraftTool { _cred_store: Arc<CredentialStore> }
impl GmailDraftTool {
    pub fn new(cred_store: Arc<CredentialStore>) -> Self { Self { _cred_store: cred_store } }
}

#[async_trait]
impl GeminiTool for GmailDraftTool {
    fn name(&self) -> &'static str { "gmail_draft" }
    fn description(&self) -> &'static str {
        "Create a draft email in the user's Gmail account for later review and sending."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "to": { "type": "string", "description": "Recipient email address" },
                "subject": { "type": "string", "description": "Email subject" },
                "body": { "type": "string", "description": "Email body (plain text)" }
            },
            "required": ["to", "subject", "body"]
        })
    }

    async fn execute(&self, input: Value, access_token: &str) -> Result<ToolResult, anyhow::Error> {
        let to = input["to"].as_str().unwrap_or("");
        let subject = input["subject"].as_str().unwrap_or("");
        let body = input["body"].as_str().unwrap_or("");

        let raw = format!("To: {}\r\nSubject: {}\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n{}", to, subject, body);
        let encoded = URL_SAFE.encode(raw.as_bytes());

        let draft_body = json!({ "message": { "raw": encoded } });
        let url = "https://gmail.googleapis.com/gmail/v1/users/me/drafts";
        let result = google_api_post(url, access_token, &draft_body).await?;

        Ok(ToolResult::success(json!({
            "draft_id": result["id"],
            "message_id": result["message"]["id"],
            "created": true
        })))
    }
}
