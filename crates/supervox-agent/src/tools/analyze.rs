use serde::{Deserialize, Serialize};
use serde_json::Value;
use sgr_agent::Llm;
use sgr_agent::agent_tool::{Tool, ToolError, ToolOutput, parse_args};
use sgr_agent::context::AgentContext;
use sgr_agent::types::{LlmConfig, Message};

use crate::types::CallAnalysis;

#[derive(Debug, Serialize, Deserialize)]
pub struct AnalyzeCallArgs {
    pub transcript: String,
}

pub struct AnalyzeCallTool {
    pub llm_config: LlmConfig,
}

#[async_trait::async_trait]
impl Tool for AnalyzeCallTool {
    fn name(&self) -> &str {
        "analyze_call"
    }

    fn description(&self) -> &str {
        "Analyze a full call transcript — summary, action items, mood, themes"
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "transcript": {
                    "type": "string",
                    "description": "Full call transcript text"
                }
            },
            "required": ["transcript"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &mut AgentContext) -> Result<ToolOutput, ToolError> {
        let args: AnalyzeCallArgs = parse_args(&args)?;
        let llm = Llm::new(&self.llm_config);

        let messages = vec![
            Message::system(
                "You are a call analysis assistant. Analyze the transcript and produce \
                 a structured analysis with: summary, action_items (each with description, \
                 optional assignee, optional deadline), decisions, open_questions, \
                 mood (positive/neutral/negative/mixed), and themes."
                    .to_string(),
            ),
            Message::user(format!("Transcript:\n{}", args.transcript)),
        ];

        let analysis: CallAnalysis = llm
            .structured(&messages)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        let json = serde_json::to_string_pretty(&analysis)
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        Ok(ToolOutput::text(json))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_analyze_args() {
        let args = serde_json::json!({
            "transcript": "Alice: Let's ship by Friday."
        });
        let parsed: AnalyzeCallArgs = parse_args(&args).unwrap();
        assert_eq!(parsed.transcript, "Alice: Let's ship by Friday.");
    }

    #[test]
    fn analyze_tool_def() {
        let tool = AnalyzeCallTool {
            llm_config: LlmConfig::auto("test"),
        };
        let def = tool.to_def();
        assert_eq!(def.name, "analyze_call");
        assert!(def.parameters["properties"]["transcript"].is_object());
    }
}
