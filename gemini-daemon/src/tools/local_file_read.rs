// tools/local_file_read.rs — Local File Read Tool (sandboxed to ~/)
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use super::{GeminiTool, ToolResult};

pub struct LocalFileReadTool {
    home_dir: PathBuf,
}

impl LocalFileReadTool {
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home".to_string());
        Self { home_dir: PathBuf::from(home) }
    }
}

#[async_trait]
impl GeminiTool for LocalFileReadTool {
    fn name(&self) -> &'static str { "local_file_read" }
    fn description(&self) -> &'static str {
        "Read the contents of a file on the user's local filesystem. Sandboxed to the user's home directory."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path relative to home directory (e.g., 'Documents/notes.txt')" },
                "max_bytes": { "type": "integer", "default": 4096, "maximum": 65536 }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: Value, _access_token: &str) -> Result<ToolResult, anyhow::Error> {
        let rel_path = input["path"].as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing path"))?;
        let max_bytes = input["max_bytes"].as_u64().unwrap_or(4096) as usize;

        // Security: prevent path traversal
        let full_path = self.home_dir.join(rel_path);
        let canonical = full_path.canonicalize()
            .map_err(|e| anyhow::anyhow!("File not found: {}", e))?;

        if !canonical.starts_with(&self.home_dir) {
            anyhow::bail!("Access denied: path traversal detected. Files must be within the home directory.");
        }

        // Check if it's a regular file
        let metadata = tokio::fs::metadata(&canonical).await?;
        if !metadata.is_file() {
            anyhow::bail!("Path is not a regular file");
        }

        // Read file content
        let content = tokio::fs::read_to_string(&canonical).await
            .map_err(|e| anyhow::anyhow!("Cannot read file (binary?): {}", e))?;

        let truncated = content.len() > max_bytes;
        let output = if truncated {
            format!("{}... [truncated at {} bytes, total {} bytes]",
                    &content[..max_bytes], max_bytes, content.len())
        } else {
            content
        };

        Ok(ToolResult::success(json!({
            "path": rel_path,
            "content": output,
            "size_bytes": metadata.len(),
            "truncated": truncated
        })))
    }
}
