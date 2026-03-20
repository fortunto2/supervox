//! Async analysis pipeline: calls LLM to analyze a call transcript.
//!
//! Decoupled from the Tool trait — calls LLM directly with structured output.
//! Returns `CallAnalysis` for mapping into `AnalysisState`.

use sgr_agent::Llm;
use sgr_agent::types::{LlmConfig, Message};
use supervox_agent::storage;
use supervox_agent::types::{CallAnalysis, CallFilter, CallInsights};

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

const INSIGHTS_TIMEOUT_SECS: u64 = 60;

/// Generate cross-call insights with optional filter.
pub async fn generate_insights_filtered(
    model: &str,
    filter: &CallFilter,
) -> Result<CallInsights, String> {
    let calls_dir = storage::default_calls_dir();
    let all_calls =
        storage::list_calls(&calls_dir).map_err(|e| format!("Failed to load calls: {e}"))?;
    let calls = storage::filter_calls(&all_calls, filter);

    if calls.is_empty() {
        return Err("No calls found matching filter criteria.".into());
    }

    let mut context = String::new();
    let mut analyzed_count = 0;
    for call in &calls {
        let date = call.created_at.format("%Y-%m-%d %H:%M");
        context.push_str(&format!(
            "--- Call ({date}, {:.0}s) ---\n",
            call.duration_secs
        ));
        match storage::load_analysis(&calls_dir, &call.id) {
            Ok(Some(analysis)) => {
                analyzed_count += 1;
                context.push_str(&format!("Summary: {}\n", analysis.summary));
                if !analysis.themes.is_empty() {
                    context.push_str(&format!("Themes: {}\n", analysis.themes.join(", ")));
                }
                if !analysis.action_items.is_empty() {
                    let items: Vec<&str> = analysis
                        .action_items
                        .iter()
                        .map(|a| a.description.as_str())
                        .collect();
                    context.push_str(&format!("Actions: {}\n", items.join("; ")));
                }
                context.push_str(&format!("Mood: {:?}\n", analysis.mood));
            }
            _ => {
                let preview: String = call.transcript.chars().take(200).collect();
                context.push_str(&format!("Transcript preview: {preview}\n"));
            }
        }
        context.push('\n');
    }

    let llm = Llm::new(&LlmConfig::auto(model));
    let messages = vec![
        Message::system(
            "You are a call analytics assistant. Analyze the following call history and \
             produce structured cross-call insights: recurring_themes (theme + count), \
             mood_summary (positive/neutral/negative/mixed counts), open_action_items \
             (still relevant items with description, optional assignee, optional deadline), \
             key_patterns (notable patterns across calls), total_calls, and period \
             (date range as a string like '2026-03-01 to 2026-03-20')."
                .to_string(),
        ),
        Message::user(format!(
            "{} calls ({} with analysis):\n\n{context}",
            calls.len(),
            analyzed_count
        )),
    ];

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(INSIGHTS_TIMEOUT_SECS),
        llm.structured(&messages),
    )
    .await;

    match result {
        Ok(Ok(insights)) => Ok(insights),
        Ok(Err(e)) => Err(format!("Insights LLM error: {e}")),
        Err(_) => Err(format!("Insights timed out after {INSIGHTS_TIMEOUT_SECS}s")),
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
