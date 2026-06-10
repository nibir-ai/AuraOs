// tools/calendar_delete.rs — Google Calendar Event Deletion (destructive)
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use crate::credentials::CredentialStore;
use super::{GeminiTool, ToolResult, google_api_delete};

pub struct CalendarDeleteTool { _cred_store: Arc<CredentialStore> }
impl CalendarDeleteTool {
    pub fn new(cred_store: Arc<CredentialStore>) -> Self { Self { _cred_store: cred_store } }
}

#[async_trait]
impl GeminiTool for CalendarDeleteTool {
    fn name(&self) -> &'static str { "calendar_delete" }
    fn description(&self) -> &'static str { "Delete a calendar event by its ID. Requires confirmation." }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "event_id": { "type": "string", "description": "The event ID to delete" }
            },
            "required": ["event_id"]
        })
    }
    fn requires_confirmation(&self) -> bool { true }

    async fn execute(&self, input: Value, access_token: &str) -> Result<ToolResult, anyhow::Error> {
        let event_id = input["event_id"].as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing event_id"))?;

        let url = format!(
            "https://www.googleapis.com/calendar/v3/calendars/primary/events/{}",
            urlencoding::encode(event_id)
        );

        google_api_delete(&url, access_token).await?;
        Ok(ToolResult::success(json!({ "deleted": true, "event_id": event_id })))
    }
}
