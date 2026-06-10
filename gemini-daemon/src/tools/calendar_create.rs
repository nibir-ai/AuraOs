// tools/calendar_create.rs — Google Calendar Event Creation (destructive)
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use crate::credentials::CredentialStore;
use super::{GeminiTool, ToolResult, google_api_post};

pub struct CalendarCreateTool { _cred_store: Arc<CredentialStore> }
impl CalendarCreateTool {
    pub fn new(cred_store: Arc<CredentialStore>) -> Self { Self { _cred_store: cred_store } }
}

#[async_trait]
impl GeminiTool for CalendarCreateTool {
    fn name(&self) -> &'static str { "calendar_create" }
    fn description(&self) -> &'static str {
        "Create a new event on the user's Google Calendar. Requires user confirmation."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "Event title/summary" },
                "start": { "type": "string", "description": "Start datetime (ISO 8601, e.g. 2025-02-11T15:00:00)" },
                "end": { "type": "string", "description": "End datetime (ISO 8601)" },
                "description": { "type": "string", "description": "Event description" },
                "location": { "type": "string", "description": "Event location" },
                "attendees": { "type": "array", "items": { "type": "string" }, "description": "List of attendee email addresses" },
                "send_invites": { "type": "boolean", "default": true, "description": "Whether to send email invitations to attendees" }
            },
            "required": ["title", "start", "end"]
        })
    }
    fn requires_confirmation(&self) -> bool { true }

    async fn execute(&self, input: Value, access_token: &str) -> Result<ToolResult, anyhow::Error> {
        let title = input["title"].as_str().unwrap_or("Untitled Event");
        let start = input["start"].as_str().unwrap_or("");
        let end = input["end"].as_str().unwrap_or("");
        let send_invites = input["send_invites"].as_bool().unwrap_or(true);

        let mut event = json!({
            "summary": title,
            "start": { "dateTime": start, "timeZone": "UTC" },
            "end": { "dateTime": end, "timeZone": "UTC" },
        });

        if let Some(desc) = input["description"].as_str() {
            event["description"] = json!(desc);
        }
        if let Some(loc) = input["location"].as_str() {
            event["location"] = json!(loc);
        }
        if let Some(attendees) = input["attendees"].as_array() {
            let att: Vec<Value> = attendees.iter()
                .filter_map(|a| a.as_str())
                .map(|email| json!({"email": email}))
                .collect();
            event["attendees"] = json!(att);
        }

        let url = format!(
            "https://www.googleapis.com/calendar/v3/calendars/primary/events?sendUpdates={}",
            if send_invites { "all" } else { "none" }
        );

        let result = google_api_post(&url, access_token, &event).await?;

        Ok(ToolResult::success(json!({
            "event_id": result["id"],
            "html_link": result["htmlLink"],
            "created": true,
            "invite_sent": send_invites
        })))
    }
}
