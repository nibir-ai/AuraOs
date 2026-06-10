// tool_registry.rs — Tool Registration and Schema Export
//
// Manages the set of tools available to Gemini for function calling.
// Each tool implements the GeminiTool trait and is registered at startup.
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use crate::cloud_client::ToolDef;
use crate::tools::GeminiTool;

/// Registry of all available Gemini tools
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn GeminiTool>>,
}

/// Builder for constructing a ToolRegistry
pub struct ToolRegistryBuilder {
    tools: HashMap<String, Box<dyn GeminiTool>>,
}

impl ToolRegistry {
    /// Create a new builder
    pub fn new() -> ToolRegistryBuilder {
        ToolRegistryBuilder {
            tools: HashMap::new(),
        }
    }

    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<&dyn GeminiTool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// Get all tool definitions for Gemini function calling
    pub fn get_tool_definitions(&self) -> Vec<ToolDef> {
        self.tools
            .values()
            .map(|tool| ToolDef {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                input_schema: tool.input_schema(),
            })
            .collect()
    }

    /// Get only non-destructive tool definitions (for read-only queries)
    pub fn get_readonly_tool_definitions(&self) -> Vec<ToolDef> {
        self.tools
            .values()
            .filter(|tool| !tool.requires_confirmation())
            .map(|tool| ToolDef {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                input_schema: tool.input_schema(),
            })
            .collect()
    }

    /// Get the number of registered tools
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// List all tool names
    pub fn tool_names(&self) -> Vec<&str> {
        self.tools.keys().map(|k| k.as_str()).collect()
    }
}

impl ToolRegistryBuilder {
    /// Register a new tool
    pub fn register(mut self, tool: Box<dyn GeminiTool>) -> Self {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
        self
    }

    /// Build the registry
    pub fn build(self) -> ToolRegistry {
        ToolRegistry {
            tools: self.tools,
        }
    }
}
