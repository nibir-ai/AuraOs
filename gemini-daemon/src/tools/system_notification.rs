// tools/system_notification.rs — Desktop Notification Tool
use async_trait::async_trait;
use serde_json::{json, Value};
use super::{GeminiTool, ToolResult};

pub struct SystemNotificationTool;
impl SystemNotificationTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl GeminiTool for SystemNotificationTool {
    fn name(&self) -> &'static str { "system_notification" }
    fn description(&self) -> &'static str {
        "Send a desktop notification to the user. Use for reminders, alerts, or status updates."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "Notification title" },
                "body": { "type": "string", "description": "Notification body text" },
                "urgency": { "type": "string", "enum": ["low", "normal", "critical"], "default": "normal" }
            },
            "required": ["title", "body"]
        })
    }

    async fn execute(&self, input: Value, _access_token: &str) -> Result<ToolResult, anyhow::Error> {
        let title = input["title"].as_str().unwrap_or("AuraOS");
        let body = input["body"].as_str().unwrap_or("");
        let urgency = input["urgency"].as_str().unwrap_or("normal");

        let output = tokio::process::Command::new("notify-send")
            .args(["--app-name=AuraOS Gemini", "--icon=auraos-gemini",
                   &format!("--urgency={}", urgency), title, body])
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => {
                Ok(ToolResult::success(json!({ "sent": true, "title": title })))
            }
            _ => Ok(ToolResult::error("Failed to send desktop notification"))
        }
    }
}
