// proactive.rs — Proactive Scheduler for Background AI Tasks
//
// Runs scheduled tasks without explicit user queries:
// - Morning briefing (daily at user's configured time)
// - Follow-up detection (every 4 hours)
// - Meeting alerts (15 minutes before calendar events)
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::cloud_client::AuraCloudClient;
use crate::credentials::CredentialStore;
use crate::tool_registry::ToolRegistry;

/// Proactive scheduler that runs background AI tasks on a schedule
pub struct ProactiveScheduler {
    tool_registry: Arc<ToolRegistry>,
    cloud_client: Arc<AuraCloudClient>,
    cred_store: Arc<CredentialStore>,
}

impl ProactiveScheduler {
    pub fn new(
        tool_registry: Arc<ToolRegistry>,
        cloud_client: Arc<AuraCloudClient>,
        cred_store: Arc<CredentialStore>,
    ) -> Self {
        Self {
            tool_registry,
            cloud_client,
            cred_store,
        }
    }

    /// Main scheduler loop — runs forever, dispatching tasks on schedule
    pub async fn run(&self) -> Result<()> {
        info!("ProactiveScheduler started");

        let mut morning_interval = tokio::time::interval(
            std::time::Duration::from_secs(3600) // Check hourly
        );
        let mut follow_up_interval = tokio::time::interval(
            std::time::Duration::from_secs(4 * 3600) // Every 4 hours
        );
        let mut meeting_interval = tokio::time::interval(
            std::time::Duration::from_secs(300) // Every 5 minutes
        );

        // Skip the initial immediate tick
        morning_interval.tick().await;
        follow_up_interval.tick().await;
        meeting_interval.tick().await;

        loop {
            tokio::select! {
                _ = morning_interval.tick() => {
                    if self.is_morning_briefing_time() {
                        if let Err(e) = self.morning_briefing().await {
                            error!("Morning briefing failed: {}", e);
                        }
                    }
                }
                _ = follow_up_interval.tick() => {
                    if let Err(e) = self.follow_up_detector().await {
                        error!("Follow-up detection failed: {}", e);
                    }
                }
                _ = meeting_interval.tick() => {
                    if let Err(e) = self.meeting_alert_check().await {
                        error!("Meeting alert check failed: {}", e);
                    }
                }
            }
        }
    }

    /// Check if it's time for the morning briefing (7:00-8:00 AM local time)
    fn is_morning_briefing_time(&self) -> bool {
        let now = chrono::Local::now();
        let hour = now.hour();
        // Only trigger once between 7:00-7:59 AM
        hour == 7
    }

    /// Morning briefing: summarize today's calendar and unread emails
    async fn morning_briefing(&self) -> Result<()> {
        info!("Running morning briefing...");

        let access_token = match self.cred_store.get_current_access_token().await {
            Ok(token) => token,
            Err(e) => {
                warn!("Cannot run morning briefing — no access token: {}", e);
                return Ok(());
            }
        };

        // Query today's calendar events
        let calendar_tool = self.tool_registry.get("calendar_query");
        let gmail_tool = self.tool_registry.get("gmail_search");

        let mut briefing_parts: Vec<String> = Vec::new();

        // Get today's events
        if let Some(tool) = calendar_tool {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            let input = serde_json::json!({
                "date": today,
                "max_results": 10
            });

            match tool.execute(input, &access_token).await {
                Ok(result) => {
                    briefing_parts.push(format!("📅 Calendar:\n{}", result.data));
                }
                Err(e) => debug!("Calendar query failed for briefing: {}", e),
            }
        }

        // Get recent unread emails
        if let Some(tool) = gmail_tool {
            let input = serde_json::json!({
                "query": "is:unread -category:promotions newer_than:12h",
                "max_results": 5
            });

            match tool.execute(input, &access_token).await {
                Ok(result) => {
                    briefing_parts.push(format!("📧 Unread Emails:\n{}", result.data));
                }
                Err(e) => debug!("Gmail query failed for briefing: {}", e),
            }
        }

        if briefing_parts.is_empty() {
            debug!("No content for morning briefing");
            return Ok(());
        }

        // Use Gemini to synthesize the briefing
        let briefing_prompt = format!(
            "Synthesize this data into a concise, friendly morning briefing for the user. \
             Keep it under 200 words.\n\n{}",
            briefing_parts.join("\n\n")
        );

        let response = self.cloud_client
            .query(
                &access_token,
                "proactive-morning-briefing",
                &[],
                &briefing_prompt,
                &[],
                "gemini-2.0-flash",
                512,
            )
            .await;

        match response {
            Ok(briefing) => {
                // Emit ProactiveInsight signal via D-Bus
                // In a real implementation, this would emit the D-Bus signal
                info!("Morning briefing generated: {}...",
                      &briefing[..briefing.len().min(100)]);

                // Send desktop notification
                send_notification(
                    "Good morning! ☀️",
                    &briefing,
                    "morning_brief",
                ).await;
            }
            Err(e) => {
                warn!("Failed to generate morning briefing: {}", e);
            }
        }

        Ok(())
    }

    /// Follow-up detector: check for sent emails without replies
    async fn follow_up_detector(&self) -> Result<()> {
        debug!("Running follow-up detection...");

        let access_token = match self.cred_store.get_current_access_token().await {
            Ok(token) => token,
            Err(_) => return Ok(()), // Skip silently if no token
        };

        let gmail_tool = self.tool_registry.get("gmail_search");
        if let Some(tool) = gmail_tool {
            let input = serde_json::json!({
                "query": "in:sent older_than:3d",
                "max_results": 5
            });

            match tool.execute(input, &access_token).await {
                Ok(result) => {
                    // Ask Gemini to identify emails that may need follow-up
                    let prompt = format!(
                        "Review these sent emails and identify any that were sent more than \
                         3 days ago and likely haven't received a reply. For each, suggest \
                         a brief follow-up message. Only mention emails that genuinely \
                         need follow-up.\n\n{}",
                        result.data
                    );

                    if let Ok(analysis) = self.cloud_client
                        .query(&access_token, "proactive-followup", &[], &prompt, &[],
                               "gemini-2.0-flash", 512)
                        .await
                    {
                        if !analysis.contains("no follow-up") && !analysis.contains("none") {
                            send_notification(
                                "Follow-up Reminder 📨",
                                &analysis,
                                "email_follow_up",
                            ).await;
                        }
                    }
                }
                Err(e) => debug!("Follow-up detection gmail query failed: {}", e),
            }
        }

        Ok(())
    }

    /// Meeting alert: check for upcoming calendar events
    async fn meeting_alert_check(&self) -> Result<()> {
        let access_token = match self.cred_store.get_current_access_token().await {
            Ok(token) => token,
            Err(_) => return Ok(()),
        };

        if let Some(tool) = self.tool_registry.get("calendar_query") {
            let now = chrono::Local::now();
            let in_15_min = now + chrono::Duration::minutes(15);

            let input = serde_json::json!({
                "time_min": now.to_rfc3339(),
                "time_max": in_15_min.to_rfc3339(),
                "max_results": 3
            });

            match tool.execute(input, &access_token).await {
                Ok(result) => {
                    let events_str = result.data.to_string();
                    if !events_str.is_empty() && events_str != "[]" && events_str != "null" {
                        send_notification(
                            "Upcoming Meeting 📅",
                            &format!("You have a meeting starting in 15 minutes:\n{}", events_str),
                            "meeting_alert",
                        ).await;
                    }
                }
                Err(e) => debug!("Meeting alert calendar query failed: {}", e),
            }
        }

        Ok(())
    }
}

/// Send a desktop notification via notify-send
async fn send_notification(title: &str, body: &str, _category: &str) {
    let result = tokio::process::Command::new("notify-send")
        .args([
            "--app-name=AuraOS Gemini",
            "--icon=auraos-gemini",
            "--urgency=normal",
            title,
            body,
        ])
        .output()
        .await;

    match result {
        Ok(output) if output.status.success() => {
            debug!("Desktop notification sent: {}", title);
        }
        Ok(output) => {
            warn!("notify-send failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Err(e) => {
            warn!("Failed to send notification: {}", e);
        }
    }
}

use chrono::Timelike;
