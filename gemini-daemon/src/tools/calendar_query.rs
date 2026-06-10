// tools/calendar_query.rs — Google Calendar Query Tool
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use crate::credentials::CredentialStore;
use super::{GeminiTool, ToolResult, google_api_get};

pub struct CalendarQueryTool { _cred_store: Arc<CredentialStore> }
impl CalendarQueryTool {
    pub fn new(cred_store: Arc<CredentialStore>) -> Self { Self { _cred_store: cred_store } }
}

#[async_trait]
impl GeminiTool for CalendarQueryTool {
    fn name(&self) -> &'static str { "calendar_query" }
    fn description(&self) -> &'static str {
        "Query the user's Google Calendar events. Can filter by date range and search terms."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "date": { "type": "string", "description": "Date to query (YYYY-MM-DD). Defaults to today." },
                "time_min": { "type": "string", "description": "Start of time range (RFC 3339)" },
                "time_max": { "type": "string", "description": "End of time range (RFC 3339)" },
                "query": { "type": "string", "description": "Free text search query" },
                "max_results": { "type": "integer", "default": 10, "maximum": 50 }
            }
        })
    }

    async fn execute(&self, input: Value, access_token: &str) -> Result<ToolResult, anyhow::Error> {
        let max_results = input["max_results"].as_u64().unwrap_or(10);
        let mut url = format!(
            "https://www.googleapis.com/calendar/v3/calendars/primary/events?maxResults={}&singleEvents=true&orderBy=startTime",
            max_results
        );

        if let Some(date) = input["date"].as_str() {
            url.push_str(&format!("&timeMin={}T00:00:00Z&timeMax={}T23:59:59Z",
                                   date, date));
        } else {
            if let Some(time_min) = input["time_min"].as_str() {
                url.push_str(&format!("&timeMin={}", urlencoding::encode(time_min)));
            }
            if let Some(time_max) = input["time_max"].as_str() {
                url.push_str(&format!("&timeMax={}", urlencoding::encode(time_max)));
            }
        }

        if let Some(query) = input["query"].as_str() {
            url.push_str(&format!("&q={}", urlencoding::encode(query)));
        }

        let result = google_api_get(&url, access_token).await?;
        let events: Vec<Value> = result["items"].as_array().cloned().unwrap_or_default()
            .into_iter()
            .map(|e| json!({
                "id": e["id"],
                "title": e["summary"],
                "start": e["start"]["dateTime"].as_str().or(e["start"]["date"].as_str()),
                "end": e["end"]["dateTime"].as_str().or(e["end"]["date"].as_str()),
                "location": e["location"],
                "description": e["description"].as_str().map(|d| if d.len() > 200 { &d[..200] } else { d }),
                "attendees": e["attendees"].as_array().map(|a| a.iter().map(|att| json!({
                    "email": att["email"],
                    "response": att["responseStatus"]
                })).collect::<Vec<_>>()),
                "hangout_link": e["hangoutLink"],
            }))
            .collect();

        Ok(ToolResult::success(json!({ "events": events, "count": events.len() })))
    }
}
