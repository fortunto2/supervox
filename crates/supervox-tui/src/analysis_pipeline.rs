//! Async analysis pipeline: calls LLM to analyze a call transcript.
//!
//! Decoupled from the Tool trait — calls LLM directly with structured output.
//! Returns `CallAnalysis` for mapping into `AnalysisState`.

use sgr_agent::Llm;
use sgr_agent::types::{LlmConfig, Message};
use supervox_agent::types::CallAnalysis;

const LLM_TIMEOUT_SECS: u64 = 30;

/// Run analysis on a call transcript. Returns structured `CallAnalysis`.
pub async fn analyze_transcript(transcript: &str, model: &str) -> Result<CallAnalysis, String> {
    let llm = Llm::new(&LlmConfig::auto(model));
    let messages = vec![
        Message::system(
            "You are a call analysis assistant. Analyze the transcript and produce \
             a structured analysis with: summary, action_items (each with description, \
             optional assignee, optional deadline), decisions, open_questions, \
             mood (positive/neutral/negative/mixed), and themes."
                .to_string(),
        ),
        Message::user(format!("Transcript:\n{transcript}")),
    ];

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(LLM_TIMEOUT_SECS),
        llm.structured(&messages),
    )
    .await;

    match result {
        Ok(Ok(analysis)) => Ok(analysis),
        Ok(Err(e)) => Err(format!("Analysis LLM error: {e}")),
        Err(_) => Err(format!("Analysis timed out after {LLM_TIMEOUT_SECS}s")),
    }
}

/// Draft a follow-up email based on analysis JSON.
pub async fn draft_follow_up(
    analysis_json: &str,
    language: &str,
    model: &str,
) -> Result<String, String> {
    let llm = Llm::new(&LlmConfig::auto(model));
    let messages = vec![
        Message::system(format!(
            "You are a professional email writer. Draft a concise follow-up email \
             based on the call analysis below. Write in {language}. \
             Include action items and next steps. Keep it professional and brief."
        )),
        Message::user(format!("Call analysis:\n{analysis_json}")),
    ];

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(LLM_TIMEOUT_SECS),
        llm.generate(&messages),
    )
    .await;

    match result {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(e)) => Err(format!("Follow-up LLM error: {e}")),
        Err(_) => Err(format!("Follow-up timed out after {LLM_TIMEOUT_SECS}s")),
    }
}

#[cfg(test)]
mod tests {
    use supervox_agent::types::{ActionItem, CallAnalysis, Mood};

    #[test]
    fn call_analysis_fields_map_to_analysis_state() {
        // Verify CallAnalysis has the fields we need for AnalysisState mapping
        let analysis = CallAnalysis {
            summary: "Test summary".into(),
            action_items: vec![ActionItem {
                description: "Do something".into(),
                assignee: Some("Alice".into()),
                deadline: None,
            }],
            follow_up_draft: None,
            decisions: vec!["Decision A".into()],
            open_questions: vec!["Question?".into()],
            mood: Mood::Positive,
            themes: vec!["topic".into()],
        };
        assert_eq!(analysis.summary, "Test summary");
        assert_eq!(analysis.action_items.len(), 1);
        assert_eq!(analysis.action_items[0].description, "Do something");
        assert_eq!(analysis.decisions.len(), 1);
        assert_eq!(analysis.open_questions.len(), 1);
        assert_eq!(analysis.mood, Mood::Positive);
        assert_eq!(analysis.themes.len(), 1);
    }

    #[test]
    fn mood_display_format() {
        assert_eq!(format!("{:?}", Mood::Positive), "Positive");
        assert_eq!(format!("{:?}", Mood::Mixed), "Mixed");
    }
}
