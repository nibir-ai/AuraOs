// tools/contacts_search.rs — Google Contacts Search Tool
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use crate::credentials::CredentialStore;
use super::{GeminiTool, ToolResult, google_api_get};

pub struct ContactsSearchTool { _cred_store: Arc<CredentialStore> }
impl ContactsSearchTool {
    pub fn new(cred_store: Arc<CredentialStore>) -> Self { Self { _cred_store: cred_store } }
}

#[async_trait]
impl GeminiTool for ContactsSearchTool {
    fn name(&self) -> &'static str { "contacts_search" }
    fn description(&self) -> &'static str {
        "Search the user's Google Contacts by name, email, or phone number."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Contact name to search for" },
                "max_results": { "type": "integer", "default": 5, "maximum": 20 }
            },
            "required": ["name"]
        })
    }

    async fn execute(&self, input: Value, access_token: &str) -> Result<ToolResult, anyhow::Error> {
        let name = input["name"].as_str().unwrap_or("");
        let max = input["max_results"].as_u64().unwrap_or(5);

        let url = format!(
            "https://people.googleapis.com/v1/people:searchContacts?query={}&readMask=names,emailAddresses,phoneNumbers,photos&pageSize={}",
            urlencoding::encode(name), max
        );

        let result = google_api_get(&url, access_token).await?;
        let contacts: Vec<Value> = result["results"].as_array().cloned().unwrap_or_default()
            .into_iter()
            .map(|r| {
                let person = &r["person"];
                let names = person["names"].as_array();
                let emails = person["emailAddresses"].as_array();
                let phones = person["phoneNumbers"].as_array();

                json!({
                    "name": names.and_then(|n| n.first())
                        .and_then(|n| n["displayName"].as_str()),
                    "email": emails.and_then(|e| e.first())
                        .and_then(|e| e["value"].as_str()),
                    "phone": phones.and_then(|p| p.first())
                        .and_then(|p| p["value"].as_str()),
                    "all_emails": emails.map(|arr| arr.iter()
                        .filter_map(|e| e["value"].as_str().map(String::from))
                        .collect::<Vec<_>>()),
                })
            })
            .collect();

        Ok(ToolResult::success(json!({ "contacts": contacts, "count": contacts.len() })))
    }
}
