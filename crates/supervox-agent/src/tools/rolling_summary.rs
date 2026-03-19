use serde::{Deserialize, Serialize};
use serde_json::Value;
use sgr_agent::Llm;
use sgr_agent::agent_tool::{Tool, ToolError, ToolOutput, parse_args};
use sgr_agent::context::AgentContext;
use sgr_agent::types::{LlmConfig, Message};

#[derive(Debug, Serialize, Deserialize)]
pub struct RollingSummaryArgs {
    pub chunks: Vec<String>,
    pub prior_summary: Option<String>,
    pub target_lang: String,
}

pub struct RollingSummaryTool {
    pub llm_config: LlmConfig,
}

#[async_trait::async_trait]
impl Tool for RollingSummaryTool {
    fn name(&self) -> &str {
        "rolling_summary"
    }

    fn description(&self) -> &str {
        "Generate a rolling summary of recent transcript chunks in the target language"
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "chunks": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Recent transcript chunks to summarize"
                },
                "prior_summary": {
                    "type": "string",
                    "description": "Previous summary for context continuity"
                },
                "target_lang": {
                    "type": "string",
                    "description": "Language for the summary output (e.g. ru, en)"
                }
            },
            "required": ["chunks", "target_lang"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &mut AgentContext) -> Result<ToolOutput, ToolError> {
        let args: RollingSummaryArgs = parse_args(&args)?;
        let llm = Llm::new(&self.llm_config);

        let transcript = args.chunks.join("\n");
        let prior = args
            .prior_summary
            .map(|s| format!("\nPrevious summary:\n{s}"))
            .unwrap_or_default();

        let messages = vec![
            Message::system(format!(
                "You are a live call summarizer. Produce 2-3 bullet points capturing \
                 the key meaning of the conversation so far. Write in {}. \
                 Focus on meaning, not word-for-word transcription. Be concise.",
                args.target_lang
            )),
            Message::user(format!("Transcript:\n{transcript}{prior}")),
        ];

        let result = llm
            .generate(&messages)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        Ok(ToolOutput::text(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rolling_summary_args() {
        let args = serde_json::json!({
            "chunks": ["Hello", "How are you?"],
            "target_lang": "ru"
        });
        let parsed: RollingSummaryArgs = parse_args(&args).unwrap();
        assert_eq!(parsed.chunks.len(), 2);
        assert!(parsed.prior_summary.is_none());
        assert_eq!(parsed.target_lang, "ru");
    }

    #[test]
    fn parse_with_prior_summary() {
        let args = serde_json::json!({
            "chunks": ["Next topic"],
            "prior_summary": "Previous: discussed budget",
            "target_lang": "en"
        });
        let parsed: RollingSummaryArgs = parse_args(&args).unwrap();
        assert_eq!(
            parsed.prior_summary.as_deref(),
            Some("Previous: discussed budget")
        );
    }

    #[test]
    fn rolling_summary_tool_def() {
        let tool = RollingSummaryTool {
            llm_config: LlmConfig::auto("test"),
        };
        let def = tool.to_def();
        assert_eq!(def.name, "rolling_summary");
        assert!(def.parameters["properties"]["chunks"].is_object());
    }
}
