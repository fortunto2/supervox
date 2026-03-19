use serde::{Deserialize, Serialize};
use serde_json::Value;
use sgr_agent::agent_tool::{Tool, ToolError, ToolOutput, parse_args};
use sgr_agent::context::AgentContext;
use std::path::{Path, PathBuf};

use crate::storage;
use crate::types::CallMatch;

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchCallsArgs {
    pub query: String,
}

pub struct SearchCallsTool {
    pub calls_dir: PathBuf,
}

#[async_trait::async_trait]
impl Tool for SearchCallsTool {
    fn name(&self) -> &str {
        "search_calls"
    }

    fn description(&self) -> &str {
        "Search past call transcripts by text query"
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query to match against call transcripts"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &mut AgentContext) -> Result<ToolOutput, ToolError> {
        let args: SearchCallsArgs = parse_args(&args)?;
        let matches = search_calls_in_dir(&self.calls_dir, &args.query)
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        let json = serde_json::to_string_pretty(&matches)
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        Ok(ToolOutput::text(json))
    }
}

/// Search saved calls for a text query. Returns matches with snippets.
pub fn search_calls_in_dir(
    calls_dir: &Path,
    query: &str,
) -> Result<Vec<CallMatch>, Box<dyn std::error::Error>> {
    let calls = storage::list_calls(calls_dir)?;
    let query_lower = query.to_lowercase();
    let mut matches = Vec::new();

    for call in calls {
        let transcript_lower = call.transcript.to_lowercase();
        if let Some(pos) = transcript_lower.find(&query_lower) {
            let start = pos.saturating_sub(50);
            let end = (pos + query.len() + 50).min(call.transcript.len());
            let snippet = call.transcript[start..end].to_string();

            // Simple relevance: count occurrences
            let count = transcript_lower.matches(&query_lower).count();
            let score = count as f64 / call.transcript.len().max(1) as f64;

            matches.push(CallMatch {
                call_id: call.id.clone(),
                snippet,
                score,
            });
        }
    }

    matches.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Call;
    use chrono::Utc;

    #[test]
    fn parse_search_args() {
        let args = serde_json::json!({"query": "budget"});
        let parsed: SearchCallsArgs = parse_args(&args).unwrap();
        assert_eq!(parsed.query, "budget");
    }

    #[test]
    fn search_tool_def() {
        let tool = SearchCallsTool {
            calls_dir: PathBuf::from("/tmp"),
        };
        let def = tool.to_def();
        assert_eq!(def.name, "search_calls");
        assert!(tool.is_read_only());
    }

    #[test]
    fn search_finds_matching_calls() {
        let tmp = tempfile::tempdir().unwrap();
        let calls_dir = tmp.path().to_path_buf();
        std::fs::create_dir_all(&calls_dir).unwrap();

        let call = Call {
            id: "test-1".into(),
            created_at: Utc::now(),
            duration_secs: 60.0,
            participants: vec![],
            language: Some("en".into()),
            transcript: "We discussed the budget allocation for Q2 and the new project timeline."
                .into(),
            translation: None,
            tags: vec![],
        };
        storage::save_call(&calls_dir, &call).unwrap();

        let results = search_calls_in_dir(&calls_dir, "budget").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].call_id, "test-1");
        assert!(results[0].snippet.contains("budget"));
    }

    #[test]
    fn search_no_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let calls_dir = tmp.path().to_path_buf();
        std::fs::create_dir_all(&calls_dir).unwrap();

        let call = Call {
            id: "test-1".into(),
            created_at: Utc::now(),
            duration_secs: 30.0,
            participants: vec![],
            language: None,
            transcript: "Hello world".into(),
            translation: None,
            tags: vec![],
        };
        storage::save_call(&calls_dir, &call).unwrap();

        let results = search_calls_in_dir(&calls_dir, "nonexistent").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let results = search_calls_in_dir(&tmp.path().to_path_buf(), "anything").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_case_insensitive() {
        let tmp = tempfile::tempdir().unwrap();
        let calls_dir = tmp.path().to_path_buf();
        std::fs::create_dir_all(&calls_dir).unwrap();

        let call = Call {
            id: "test-1".into(),
            created_at: Utc::now(),
            duration_secs: 10.0,
            participants: vec![],
            language: None,
            transcript: "Budget BUDGET budget".into(),
            translation: None,
            tags: vec![],
        };
        storage::save_call(&calls_dir, &call).unwrap();

        let results = search_calls_in_dir(&calls_dir, "BUDGET").unwrap();
        assert_eq!(results.len(), 1);
    }
}
