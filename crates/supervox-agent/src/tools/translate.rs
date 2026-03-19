use serde::{Deserialize, Serialize};
use serde_json::Value;
use sgr_agent::Llm;
use sgr_agent::agent_tool::{Tool, ToolError, ToolOutput, parse_args};
use sgr_agent::context::AgentContext;
use sgr_agent::types::{LlmConfig, Message};

#[derive(Debug, Serialize, Deserialize)]
pub struct TranslateArgs {
    pub text: String,
    pub from_lang: String,
    pub to_lang: String,
}

pub struct TranslateTool {
    pub llm_config: LlmConfig,
}

#[async_trait::async_trait]
impl Tool for TranslateTool {
    fn name(&self) -> &str {
        "translate"
    }

    fn description(&self) -> &str {
        "Translate text from one language to another"
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "Text to translate" },
                "from_lang": { "type": "string", "description": "Source language code (e.g. en, ru)" },
                "to_lang": { "type": "string", "description": "Target language code (e.g. en, ru)" }
            },
            "required": ["text", "from_lang", "to_lang"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &mut AgentContext) -> Result<ToolOutput, ToolError> {
        let args: TranslateArgs = parse_args(&args)?;
        let llm = Llm::new(&self.llm_config);
        let messages = vec![
            Message::system(format!(
                "You are a translator. Translate the text from {} to {}. \
                 Return ONLY the translated text, no explanations.",
                args.from_lang, args.to_lang
            )),
            Message::user(&args.text),
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
    fn parse_translate_args() {
        let args = serde_json::json!({
            "text": "Hello",
            "from_lang": "en",
            "to_lang": "ru"
        });
        let parsed: TranslateArgs = parse_args(&args).unwrap();
        assert_eq!(parsed.text, "Hello");
        assert_eq!(parsed.from_lang, "en");
        assert_eq!(parsed.to_lang, "ru");
    }

    #[test]
    fn translate_tool_def() {
        let tool = TranslateTool {
            llm_config: LlmConfig::auto("test"),
        };
        let def = tool.to_def();
        assert_eq!(def.name, "translate");
        assert!(def.parameters["properties"]["text"].is_object());
        assert!(def.parameters["properties"]["from_lang"].is_object());
        assert!(def.parameters["properties"]["to_lang"].is_object());
    }

    #[test]
    fn translate_missing_args_fails() {
        let args = serde_json::json!({"text": "Hello"});
        let result = parse_args::<TranslateArgs>(&args);
        assert!(result.is_err());
    }
}
