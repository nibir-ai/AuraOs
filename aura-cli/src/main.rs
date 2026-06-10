// aura-cli — Command-Line Interface to Gemini via D-Bus
//
// Provides a terminal-based interface to the Gemini assistant,
// communicating with gemini-daemon over D-Bus.
//
// Usage:
//   aura-cli "What meetings do I have today?"
//   aura-cli --task "Schedule a call with Alice"
//   aura-cli --history
//   aura-cli --clear
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use clap::Parser;
use colored::*;
use std::collections::HashMap;
use zbus::Connection;
use zbus::zvariant::Value;

#[derive(Parser, Debug)]
#[command(
    name = "aura-cli",
    about = "AuraOS Gemini Assistant — Command Line Interface",
    version,
    long_about = "Talk to Gemini from your terminal. Queries gemini-daemon over D-Bus."
)]
struct Args {
    /// Query or task description (positional)
    prompt: Option<String>,

    /// Run as an agentic task (multi-step, uses tools)
    #[arg(short, long)]
    task: bool,

    /// Show conversation history
    #[arg(long)]
    history: bool,

    /// Clear conversation history
    #[arg(long)]
    clear: bool,

    /// Check status of a running task
    #[arg(long)]
    status: Option<String>,

    /// Cancel a running task
    #[arg(long)]
    cancel: Option<String>,

    /// Model to use (default: gemini-2.0-flash)
    #[arg(short, long, default_value = "gemini-2.0-flash")]
    model: String,

    /// Interactive mode (multi-turn conversation)
    #[arg(short, long)]
    interactive: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Connect to D-Bus session bus
    let conn = Connection::session()
        .await
        .context("Failed to connect to D-Bus session bus. Is gemini-daemon running?")?;

    let proxy = GeminiProxy::new(&conn).await?;

    if args.history {
        return show_history(&proxy).await;
    }

    if args.clear {
        return clear_history(&proxy).await;
    }

    if let Some(ref task_id) = args.status {
        return check_status(&proxy, task_id).await;
    }

    if let Some(ref task_id) = args.cancel {
        return cancel_task(&proxy, task_id).await;
    }

    if args.interactive {
        return interactive_mode(&proxy, &args.model).await;
    }

    // Single query or task
    let prompt = args.prompt
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("No prompt provided. Use: aura-cli \"your question\""))?;

    if args.task {
        dispatch_task(&proxy, prompt).await
    } else {
        query(&proxy, prompt, &args.model).await
    }
}

/// D-Bus proxy for com.auraos.GeminiAssistant1
struct GeminiProxy<'a> {
    proxy: zbus::Proxy<'a>,
}

impl<'a> GeminiProxy<'a> {
    async fn new(conn: &'a Connection) -> Result<GeminiProxy<'a>> {
        let proxy = zbus::ProxyBuilder::new(conn)
            .destination("com.auraos.GeminiAssistant")?
            .path("/com/auraos/GeminiAssistant")?
            .interface("com.auraos.GeminiAssistant1")?
            .build()
            .await
            .context("Failed to create D-Bus proxy. Is gemini-daemon running?")?;

        Ok(GeminiProxy { proxy })
    }

    async fn query(&self, prompt: &str, model: &str) -> Result<(String, String)> {
        let mut options: HashMap<String, Value<'_>> = HashMap::new();
        options.insert("model".to_string(), Value::from(model));

        let (response, task_id): (String, String) = self.proxy
            .call("Query", &(prompt, options))
            .await
            .context("Query call failed")?;

        Ok((response, task_id))
    }

    async fn dispatch_task(&self, description: &str) -> Result<String> {
        let context = "{}"; // Empty context
        let task_id: String = self.proxy
            .call("DispatchTask", &(description, context))
            .await
            .context("DispatchTask call failed")?;

        Ok(task_id)
    }

    async fn get_task_status(&self, task_id: &str) -> Result<(String, String)> {
        let (status, result): (String, String) = self.proxy
            .call("GetTaskStatus", &(task_id,))
            .await?;

        Ok((status, result))
    }

    async fn cancel_task(&self, task_id: &str) -> Result<bool> {
        let success: bool = self.proxy
            .call("CancelTask", &(task_id,))
            .await?;

        Ok(success)
    }

    async fn get_history(&self) -> Result<String> {
        let history: String = self.proxy
            .call("GetConversationHistory", &())
            .await?;

        Ok(history)
    }

    async fn clear_conversation(&self) -> Result<()> {
        self.proxy
            .call::<_, ()>("ClearConversation", &())
            .await?;

        Ok(())
    }
}

/// Execute a single synchronous query
async fn query(proxy: &GeminiProxy<'_>, prompt: &str, model: &str) -> Result<()> {
    print!("{}", "⏳ Thinking...".dimmed());

    let (response, _task_id) = proxy.query(prompt, model).await?;

    // Clear the "Thinking..." line
    print!("\r{}\r", " ".repeat(30));

    println!("{}", "Gemini".cyan().bold());
    println!("{}", "─".repeat(60).dimmed());
    println!("{}", response);
    println!();

    Ok(())
}

/// Dispatch an agentic task
async fn dispatch_task(proxy: &GeminiProxy<'_>, description: &str) -> Result<()> {
    println!("{} {}", "📋 Dispatching task:".yellow().bold(), description);

    let task_id = proxy.dispatch_task(description).await?;

    println!("{} {}", "Task ID:".dimmed(), task_id.green());
    println!("{}", "Task is running in the background.".dimmed());
    println!("Check status: {} {}", "aura-cli --status".cyan(), task_id);

    // Poll for completion
    println!("\n{}", "Waiting for completion...".dimmed());
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let (status, result) = proxy.get_task_status(&task_id).await?;

        match status.as_str() {
            "completed" => {
                println!("\n{}", "✓ Task completed!".green().bold());
                println!("{}", "─".repeat(60).dimmed());
                println!("{}", result);
                break;
            }
            "failed" => {
                println!("\n{}", "✗ Task failed.".red().bold());
                println!("{}", result);
                break;
            }
            "cancelled" => {
                println!("\n{}", "⊘ Task cancelled.".yellow());
                break;
            }
            _ => {
                print!(".");
            }
        }
    }

    Ok(())
}

/// Show conversation history
async fn show_history(proxy: &GeminiProxy<'_>) -> Result<()> {
    let history_json = proxy.get_history().await?;
    let history: Vec<serde_json::Value> = serde_json::from_str(&history_json)?;

    if history.is_empty() {
        println!("{}", "No conversation history.".dimmed());
        return Ok(());
    }

    println!("{}", "Conversation History".cyan().bold());
    println!("{}", "═".repeat(60).dimmed());

    for msg in &history {
        let role = msg["role"].as_str().unwrap_or("?");
        let content = msg["content"].as_str().unwrap_or("");
        let timestamp = msg["timestamp"].as_str().unwrap_or("");

        match role {
            "user" => {
                println!("\n{} {}", "You".green().bold(), timestamp.dimmed());
                println!("{}", content);
            }
            "model" => {
                println!("\n{} {}", "Gemini".cyan().bold(), timestamp.dimmed());
                println!("{}", content);
            }
            "tool" => {
                println!("\n{} {}", "Tool".yellow(), timestamp.dimmed());
                println!("{}", content.dimmed());
            }
            _ => {}
        }
    }

    println!();
    Ok(())
}

/// Clear conversation history
async fn clear_history(proxy: &GeminiProxy<'_>) -> Result<()> {
    proxy.clear_conversation().await?;
    println!("{}", "✓ Conversation history cleared.".green());
    Ok(())
}

/// Check task status
async fn check_status(proxy: &GeminiProxy<'_>, task_id: &str) -> Result<()> {
    let (status, result) = proxy.get_task_status(task_id).await?;

    println!("{} {}", "Task:".dimmed(), task_id);
    println!("{} {}", "Status:".dimmed(), match status.as_str() {
        "completed" => status.green(),
        "running" | "pending" => status.yellow(),
        "failed" => status.red(),
        _ => status.normal(),
    });

    if !result.is_empty() {
        println!("{}", "─".repeat(60).dimmed());
        println!("{}", result);
    }

    Ok(())
}

/// Cancel a running task
async fn cancel_task(proxy: &GeminiProxy<'_>, task_id: &str) -> Result<()> {
    let success = proxy.cancel_task(task_id).await?;
    if success {
        println!("{}", "✓ Task cancelled.".green());
    } else {
        println!("{}", "Task not found or already completed.".yellow());
    }
    Ok(())
}

/// Interactive multi-turn conversation mode
async fn interactive_mode(proxy: &GeminiProxy<'_>, model: &str) -> Result<()> {
    println!("{}", "AuraOS Gemini — Interactive Mode".cyan().bold());
    println!("{}", "Type your messages. Use 'exit' or Ctrl+C to quit.".dimmed());
    println!("{}", "─".repeat(60).dimmed());

    let stdin = std::io::stdin();
    let mut input = String::new();

    loop {
        print!("\n{} ", "You >".green().bold());
        std::io::Write::flush(&mut std::io::stdout())?;

        input.clear();
        if stdin.read_line(&mut input)? == 0 {
            break; // EOF
        }

        let prompt = input.trim();
        if prompt.is_empty() {
            continue;
        }
        if prompt == "exit" || prompt == "quit" {
            println!("{}", "Goodbye! 👋".dimmed());
            break;
        }
        if prompt == "/clear" {
            proxy.clear_conversation().await?;
            println!("{}", "History cleared.".dimmed());
            continue;
        }

        match proxy.query(prompt, model).await {
            Ok((response, _)) => {
                println!("\n{}", "Gemini >".cyan().bold());
                println!("{}", response);
            }
            Err(e) => {
                eprintln!("{} {}", "Error:".red().bold(), e);
            }
        }
    }

    Ok(())
}
