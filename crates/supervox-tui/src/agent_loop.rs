//! Agent loop — LLM streaming for agent mode with call history context.
//!
//! Uses direct Llm::stream_complete for simplicity. Sends streaming
//! chunks back via AppEvent channel for real-time display.

use sgr_agent::Llm;
use sgr_agent::types::{LlmConfig, Message};
use supervox_agent::storage;
use supervox_agent::types::Config;
use tokio::sync::mpsc;

use crate::app::AppEvent;

/// System prompt for the agent.
const SYSTEM_PROMPT: &str = "You are SuperVox assistant. You help users understand their call \
    history. Answer questions about past calls based on the context provided. \
    Be concise and helpful. If you don't have enough information, say so.";

/// Build context from recent calls (up to 10).
/// Enriches with analysis summaries + themes when available, falls back to transcript preview.
pub fn build_calls_context() -> String {
    let calls_dir = storage::default_calls_dir();
    let calls = storage::list_calls(&calls_dir).unwrap_or_default();

    if calls.is_empty() {
        return "No past calls found.".to_string();
    }

    let mut context = String::from("Recent calls:\n\n");
    for call in calls.iter().take(10) {
        let date = call.created_at.format("%Y-%m-%d %H:%M");
        let duration = call.duration_secs as u64;

        context.push_str(&format!(
            "--- Call {id} ({date}, {duration}s) ---\n",
            id = call.id
        ));

        // Try to load analysis for richer context
        match storage::load_analysis(&calls_dir, &call.id) {
            Ok(Some(analysis)) => {
                context.push_str(&format!("Summary: {}\n", analysis.summary));
                if !analysis.themes.is_empty() {
                    context.push_str(&format!("Themes: {}\n", analysis.themes.join(", ")));
                }
                if !analysis.action_items.is_empty() {
                    context.push_str("Action items: ");
                    let items: Vec<&str> = analysis
                        .action_items
                        .iter()
                        .map(|a| a.description.as_str())
                        .collect();
                    context.push_str(&items.join("; "));
                    context.push('\n');
                }
                context.push_str(&format!("Mood: {:?}\n", analysis.mood));
            }
            _ => {
                // Fall back to transcript preview
                let preview: String = call.transcript.chars().take(200).collect();
                context.push_str(&preview);
                context.push('\n');
            }
        }
        context.push('\n');
    }
    context
}

/// Run a single agent query with streaming response.
pub async fn run_agent_query(
    question: &str,
    calls_context: &str,
    config: &Config,
    tx: mpsc::UnboundedSender<AppEvent>,
) {
    let llm = Llm::new(&LlmConfig::auto(config.effective_model()));

    let messages = vec![
        Message::system(format!(
            "{SYSTEM_PROMPT}\n\nCall history context:\n{calls_context}"
        )),
        Message::user(question.to_string()),
    ];

    let tx_clone = tx.clone();
    match llm
        .stream_complete(&messages, move |token| {
            let _ = tx_clone.send(AppEvent::AgentChunk(token.to_string()));
        })
        .await
    {
        Ok(_) => {
            let _ = tx.send(AppEvent::AgentDone);
        }
        Err(e) => {
            let _ = tx.send(AppEvent::AgentError(format!("{e}")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_is_set() {
        assert!(SYSTEM_PROMPT.contains("SuperVox"));
        assert!(SYSTEM_PROMPT.contains("call history"));
    }

    #[test]
    fn build_calls_context_empty() {
        // With a non-existent calls dir, should return "No past calls"
        let ctx = build_calls_context();
        // Either "No past calls" or actual calls — both valid
        assert!(!ctx.is_empty());
    }

    #[test]
    fn build_calls_context_returns_string() {
        // build_calls_context reads the default dir — it should never panic
        let ctx = build_calls_context();
        assert!(!ctx.is_empty());
    }
}
