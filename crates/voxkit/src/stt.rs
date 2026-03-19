//! Speech-to-text trait — provider-agnostic interface.

use crate::types::{AudioChunk, Transcript};

/// Errors from STT operations.
#[derive(Debug, thiserror::Error)]
pub enum SttError {
    /// Audio encoding failed.
    #[error("Audio encoding: {0}")]
    Encoding(String),
    /// Network/API request failed.
    #[error("Request: {0}")]
    Request(String),
    /// API returned an error response.
    #[error("API error ({status}): {body}")]
    Api { status: u16, body: String },
    /// Transcription result was empty.
    #[error("Empty transcription")]
    Empty,
    /// Backend-specific error.
    #[error("{0}")]
    Other(String),
}

/// Speech-to-text backend.
///
/// Implementations: `OpenAiStt` (feature `openai`), local Whisper (feature `whisper`).
#[async_trait::async_trait]
pub trait SttBackend: Send + Sync {
    /// Transcribe an audio chunk.
    async fn transcribe(&self, audio: &AudioChunk) -> Result<Transcript, SttError>;

    /// Transcribe with conversation context (improves accuracy for follow-up turns).
    async fn transcribe_with_context(
        &self,
        audio: &AudioChunk,
        context: Option<&str>,
    ) -> Result<Transcript, SttError> {
        // Default: ignore context
        let _ = context;
        self.transcribe(audio).await
    }

    /// Backend name for logging/diagnostics.
    fn name(&self) -> &str;
}

/// Streaming transcript event (for real-time STT backends).
#[derive(Debug, Clone)]
pub enum TranscriptEvent {
    /// Partial/incremental transcript (updated as user speaks).
    Delta { item_id: String, text: String },
    /// Final transcript for a completed speech turn.
    Final { item_id: String, text: String },
    /// Provider error.
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stt_error_display() {
        let e = SttError::Api {
            status: 429,
            body: "rate limited".into(),
        };
        assert_eq!(format!("{e}"), "API error (429): rate limited");
    }

    #[test]
    fn stt_error_empty() {
        let e = SttError::Empty;
        assert_eq!(format!("{e}"), "Empty transcription");
    }

    #[test]
    fn transcript_event_variants() {
        let delta = TranscriptEvent::Delta {
            item_id: "x".into(),
            text: "hel".into(),
        };
        let final_ = TranscriptEvent::Final {
            item_id: "x".into(),
            text: "hello".into(),
        };
        let err = TranscriptEvent::Error("fail".into());

        // Just verify construction works
        assert!(matches!(delta, TranscriptEvent::Delta { .. }));
        assert!(matches!(final_, TranscriptEvent::Final { .. }));
        assert!(matches!(err, TranscriptEvent::Error(_)));
    }
}
