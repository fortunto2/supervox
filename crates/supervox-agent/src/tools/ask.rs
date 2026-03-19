use serde::{Deserialize, Serialize};
use serde_json::Value;
use sgr_agent::Llm;
use sgr_agent::agent_tool::{Tool, ToolError, ToolOutput, parse_args};
use sgr_agent::context::AgentContext;
use sgr_agent::types::{LlmConfig, Message};

#[derive(Debug, Serialize, Deserialize)]
pub struct AskAboutCallsArgs {
    pub question: String,
    pub context: String,
}

pub struct AskAboutCallsTool {
    pub llm_config: LlmConfig,
}

#[async_trait::async_trait]
impl Tool for AskAboutCallsTool {
    fn name(&self) -> &str {
        "ask_about_calls"
    }

    fn description(&self) -> &str {
        "Answer questions about call history using provided call context"
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "Question about call history"
                },
                "context": {
                    "type": "string",
                    "description": "Relevant call transcripts/summaries as context"
                }
            },
            "required": ["question", "context"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &mut AgentContext) -> Result<ToolOutput, ToolError> {
        let args: AskAboutCallsArgs = parse_args(&args)?;
        let llm = Llm::new(&self.llm_config);

        let messages = vec![
            Message::system(
                "You are a helpful assistant that answers questions about past calls. \
                 Use the provided call context to give accurate, concise answers. \
                 If the context doesn't contain enough information, say so."
                    .to_string(),
            ),
            Message::user(format!(
                "Call context:\n{}\n\nQuestion: {}",
                args.context, args.question
            )),
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
    fn parse_ask_args() {
        let args = serde_json::json!({
            "question": "What was decided about the budget?",
            "context": "Alice: We should allocate 50k. Bob: Agreed."
        });
        let parsed: AskAboutCallsArgs = parse_args(&args).unwrap();
        assert_eq!(parsed.question, "What was decided about the budget?");
        assert!(parsed.context.contains("50k"));
    }

    #[test]
    fn ask_tool_def() {
        let tool = AskAboutCallsTool {
            llm_config: LlmConfig::auto("test"),
        };
        let def = tool.to_def();
        assert_eq!(def.name, "ask_about_calls");
        assert!(def.parameters["properties"]["question"].is_object());
        assert!(def.parameters["properties"]["context"].is_object());
    }
}
