// tools/gmail_send.rs — Gmail Send Tool (destructive — requires confirmation)
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use crate::credentials::CredentialStore;
use super::{GeminiTool, ToolResult, google_api_post};
use base64::{engine::general_purpose::URL_SAFE, Engine};

pub struct GmailSendTool { _cred_store: Arc<CredentialStore> }

impl GmailSendTool {
    pub fn new(cred_store: Arc<CredentialStore>) -> Self {
        Self { _cred_store: cred_store }
    }
}

#[async_trait]
impl GeminiTool for GmailSendTool {
    fn name(&self) -> &'static str { "gmail_send" }
    fn description(&self) -> &'static str {
        "Send an email from the user's Gmail account. Requires user confirmation."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "to": { "type": "string", "description": "Recipient email address" },
                "subject": { "type": "string", "description": "Email subject" },
                "body": { "type": "string", "description": "Email body (plain text)" },
                "cc": { "type": "string", "description": "CC recipients (comma-separated)" },
                "bcc": { "type": "string", "description": "BCC recipients (comma-separated)" }
            },
            "required": ["to", "subject", "body"]
        })
    }
    fn requires_confirmation(&self) -> bool { true }

    async fn execute(&self, input: Value, access_token: &str) -> Result<ToolResult, anyhow::Error> {
        let to = input["to"].as_str().unwrap_or("");
        let subject = input["subject"].as_str().unwrap_or("");
        let body = input["body"].as_str().unwrap_or("");
        let cc = input["cc"].as_str().unwrap_or("");
        let bcc = input["bcc"].as_str().unwrap_or("");

        // Build RFC 2822 message
        let mut raw_message = format!("To: {}\r\nSubject: {}\r\n", to, subject);
        if !cc.is_empty() { raw_message.push_str(&format!("Cc: {}\r\n", cc)); }
        if !bcc.is_empty() { raw_message.push_str(&format!("Bcc: {}\r\n", bcc)); }
        raw_message.push_str("Content-Type: text/plain; charset=UTF-8\r\n\r\n");
        raw_message.push_str(body);

        let encoded = URL_SAFE.encode(raw_message.as_bytes());
        let send_body = json!({ "raw": encoded });

        let url = "https://gmail.googleapis.com/gmail/v1/users/me/messages/send";
        let result = google_api_post(url, access_token, &send_body).await?;

        Ok(ToolResult::success(json!({
            "message_id": result["id"],
            "thread_id": result["threadId"],
            "sent": true
        })))
    }
}
