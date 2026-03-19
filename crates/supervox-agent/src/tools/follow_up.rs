use serde::{Deserialize, Serialize};
use serde_json::Value;
use sgr_agent::Llm;
use sgr_agent::agent_tool::{Tool, ToolError, ToolOutput, parse_args};
use sgr_agent::context::AgentContext;
use sgr_agent::types::{LlmConfig, Message};

#[derive(Debug, Serialize, Deserialize)]
pub struct DraftFollowUpArgs {
    pub analysis_json: String,
    pub language: String,
}

pub struct DraftFollowUpTool {
    pub llm_config: LlmConfig,
}

#[async_trait::async_trait]
impl Tool for DraftFollowUpTool {
    fn name(&self) -> &str {
        "draft_follow_up"
    }

    fn description(&self) -> &str {
        "Draft a follow-up email based on call analysis"
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "analysis_json": {
                    "type": "string",
                    "description": "JSON string of CallAnalysis output from analyze_call"
                },
                "language": {
                    "type": "string",
                    "description": "Language for the email (e.g. en, ru, auto)"
                }
            },
            "required": ["analysis_json", "language"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &mut AgentContext) -> Result<ToolOutput, ToolError> {
        let args: DraftFollowUpArgs = parse_args(&args)?;
        let llm = Llm::new(&self.llm_config);

        let messages = vec![
            Message::system(format!(
                "You are a professional email writer. Draft a concise follow-up email \
                 based on the call analysis below. Write in {}. \
                 Include action items and next steps. Keep it professional and brief.",
                args.language
            )),
            Message::user(format!("Call analysis:\n{}", args.analysis_json)),
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
    fn parse_follow_up_args() {
        let args = serde_json::json!({
            "analysis_json": "{\"summary\": \"test\"}",
            "language": "en"
        });
        let parsed: DraftFollowUpArgs = parse_args(&args).unwrap();
        assert_eq!(parsed.language, "en");
        assert!(parsed.analysis_json.contains("summary"));
    }

    #[test]
    fn follow_up_tool_def() {
        let tool = DraftFollowUpTool {
            llm_config: LlmConfig::auto("test"),
        };
        let def = tool.to_def();
        assert_eq!(def.name, "draft_follow_up");
    }
}
