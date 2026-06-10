// tools/gmail_search.rs — Gmail Search Tool
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use crate::credentials::CredentialStore;
use super::{GeminiTool, ToolResult, google_api_get};

pub struct GmailSearchTool {
    _cred_store: Arc<CredentialStore>,
}

impl GmailSearchTool {
    pub fn new(cred_store: Arc<CredentialStore>) -> Self {
        Self { _cred_store: cred_store }
    }
}

#[async_trait]
impl GeminiTool for GmailSearchTool {
    fn name(&self) -> &'static str { "gmail_search" }

    fn description(&self) -> &'static str {
        "Search the user's Gmail inbox. Returns matching emails with sender, subject, date, and snippet."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Gmail search query (e.g., 'from:boss@company.com subject:urgent')"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return",
                    "default": 5,
                    "maximum": 20
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value, access_token: &str) -> Result<ToolResult, anyhow::Error> {
        let query = input["query"].as_str().unwrap_or("");
        let max_results = input["max_results"].as_u64().unwrap_or(5);

        // Search for messages
        let url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/messages?q={}&maxResults={}",
            urlencoding::encode(query), max_results
        );

        let search_result = google_api_get(&url, access_token).await?;

        let messages = search_result["messages"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        // Fetch details for each message
        let mut emails = Vec::new();
        for msg in messages.iter().take(max_results as usize) {
            if let Some(id) = msg["id"].as_str() {
                let detail_url = format!(
                    "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=metadata&metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date",
                    id
                );

                if let Ok(detail) = google_api_get(&detail_url, access_token).await {
                    let headers = detail["payload"]["headers"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();

                    let mut email = json!({
                        "id": id,
                        "snippet": detail["snippet"].as_str().unwrap_or(""),
                    });

                    for header in headers {
                        let name = header["name"].as_str().unwrap_or("");
                        let value = header["value"].as_str().unwrap_or("");
                        match name {
                            "From" => { email["from"] = json!(value); }
                            "Subject" => { email["subject"] = json!(value); }
                            "Date" => { email["date"] = json!(value); }
                            _ => {}
                        }
                    }

                    emails.push(email);
                }
            }
        }

        Ok(ToolResult::success(json!({
            "emails": emails,
            "total_results": messages.len(),
            "query": query
        })))
    }
}
