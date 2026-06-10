// conversation.rs — Conversation History Manager
//
// SQLite-backed multi-turn conversation persistence. Each user has their
// own conversation history, stored in /var/lib/auraos/gemini-conversations.db.
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

/// A single message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,      // "user", "model", or "tool"
    pub content: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<ToolCallRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_response: Option<ToolResponseRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub arguments: String,
    pub call_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResponseRecord {
    pub call_id: String,
    pub result: String,
    pub success: bool,
}

/// Manages conversation history in SQLite
pub struct ConversationManager {
    db: Arc<Mutex<Connection>>,
}

impl ConversationManager {
    /// Create a new ConversationManager with SQLite storage
    pub async fn new(db_path: &str) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let conn = Connection::open(db_path)
            .context("Failed to open conversation database")?;

        // Create tables
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                tool_call_json TEXT,
                tool_response_json TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (conversation_id) REFERENCES conversations(id)
            );

            CREATE INDEX IF NOT EXISTS idx_messages_conv
                ON messages(conversation_id, created_at);
            "
        ).context("Failed to create conversation tables")?;

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }

    /// Add a user message to a conversation
    pub async fn add_user_message(&self, conversation_id: &str, content: &str) -> Result<()> {
        let db = self.db.lock().await;
        self.ensure_conversation(&db, conversation_id)?;

        db.execute(
            "INSERT INTO messages (conversation_id, role, content) VALUES (?1, 'user', ?2)",
            params![conversation_id, content],
        ).context("Failed to insert user message")?;

        debug!("User message added to conversation {}", conversation_id);
        Ok(())
    }

    /// Add an assistant (model) message to a conversation
    pub async fn add_assistant_message(&self, conversation_id: &str, content: &str) -> Result<()> {
        let db = self.db.lock().await;
        self.ensure_conversation(&db, conversation_id)?;

        db.execute(
            "INSERT INTO messages (conversation_id, role, content) VALUES (?1, 'model', ?2)",
            params![conversation_id, content],
        ).context("Failed to insert assistant message")?;

        debug!("Assistant message added to conversation {}", conversation_id);
        Ok(())
    }

    /// Add a tool call record
    pub async fn add_tool_call(
        &self,
        conversation_id: &str,
        tool_name: &str,
        arguments: &str,
        call_id: &str,
    ) -> Result<()> {
        let db = self.db.lock().await;

        let tool_call = ToolCallRecord {
            tool_name: tool_name.to_string(),
            arguments: arguments.to_string(),
            call_id: call_id.to_string(),
        };
        let tool_call_json = serde_json::to_string(&tool_call)?;

        db.execute(
            "INSERT INTO messages (conversation_id, role, content, tool_call_json) \
             VALUES (?1, 'model', ?2, ?3)",
            params![
                conversation_id,
                format!("Calling tool: {}", tool_name),
                tool_call_json,
            ],
        )?;

        Ok(())
    }

    /// Add a tool response record
    pub async fn add_tool_response(
        &self,
        conversation_id: &str,
        call_id: &str,
        result: &str,
        success: bool,
    ) -> Result<()> {
        let db = self.db.lock().await;

        let tool_response = ToolResponseRecord {
            call_id: call_id.to_string(),
            result: result.to_string(),
            success,
        };
        let tool_response_json = serde_json::to_string(&tool_response)?;

        db.execute(
            "INSERT INTO messages (conversation_id, role, content, tool_response_json) \
             VALUES (?1, 'tool', ?2, ?3)",
            params![
                conversation_id,
                result,
                tool_response_json,
            ],
        )?;

        Ok(())
    }

    /// Get conversation history for a specific conversation
    pub async fn get_history(&self, conversation_id: &str) -> Result<Vec<Message>> {
        let db = self.db.lock().await;

        let mut stmt = db.prepare(
            "SELECT role, content, tool_call_json, tool_response_json, created_at \
             FROM messages WHERE conversation_id = ?1 ORDER BY created_at ASC"
        )?;

        let messages = stmt
            .query_map(params![conversation_id], |row| {
                let role: String = row.get(0)?;
                let content: String = row.get(1)?;
                let tool_call_json: Option<String> = row.get(2)?;
                let tool_response_json: Option<String> = row.get(3)?;
                let timestamp: String = row.get(4)?;

                let tool_call = tool_call_json
                    .and_then(|j| serde_json::from_str(&j).ok());
                let tool_response = tool_response_json
                    .and_then(|j| serde_json::from_str(&j).ok());

                Ok(Message {
                    role,
                    content,
                    timestamp,
                    tool_call,
                    tool_response,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to read conversation history")?;

        Ok(messages)
    }

    /// Get all conversation history (for D-Bus GetConversationHistory)
    pub async fn get_all_history(&self) -> Result<Vec<Message>> {
        let db = self.db.lock().await;

        let mut stmt = db.prepare(
            "SELECT role, content, tool_call_json, tool_response_json, created_at \
             FROM messages ORDER BY created_at ASC LIMIT 1000"
        )?;

        let messages = stmt
            .query_map([], |row| {
                let role: String = row.get(0)?;
                let content: String = row.get(1)?;
                let tool_call_json: Option<String> = row.get(2)?;
                let tool_response_json: Option<String> = row.get(3)?;
                let timestamp: String = row.get(4)?;

                let tool_call = tool_call_json
                    .and_then(|j| serde_json::from_str(&j).ok());
                let tool_response = tool_response_json
                    .and_then(|j| serde_json::from_str(&j).ok());

                Ok(Message {
                    role,
                    content,
                    timestamp,
                    tool_call,
                    tool_response,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to read all history")?;

        Ok(messages)
    }

    /// Clear all conversation history
    pub async fn clear_all(&self) -> Result<()> {
        let db = self.db.lock().await;
        db.execute_batch("DELETE FROM messages; DELETE FROM conversations;")?;
        debug!("All conversation history cleared");
        Ok(())
    }

    /// Ensure a conversation record exists
    fn ensure_conversation(&self, db: &Connection, conversation_id: &str) -> Result<()> {
        db.execute(
            "INSERT OR IGNORE INTO conversations (id) VALUES (?1)",
            params![conversation_id],
        )?;
        Ok(())
    }
}
