//! Speech-to-text trait — provider-agnostic interface.

use crate::types::{AudioChunk, Transcript};
use tokio::sync::mpsc;

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
// async_trait adds #[must_use] to a boxed future; clippy 1.99 flags that as double_must_use.
#[allow(clippy::double_must_use)]
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

/// Who spoke a finalized segment (local backends with diarization).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpeakerId {
    /// The agent's own voice, heard back through the loudspeaker.
    Agent,
    /// An anonymous voice cluster, 1-based (`S<n>`).
    Cluster(usize),
    /// A voice that matched a voiceprint.
    Named(String),
}

impl SpeakerId {
    /// Text shown for this speaker; `agent_name` is what the agent is called.
    pub fn label(&self, agent_name: &str) -> String {
        match self {
            SpeakerId::Agent => agent_name.to_string(),
            SpeakerId::Cluster(n) => format!("S{n}"),
            SpeakerId::Named(name) => name.clone(),
        }
    }
}

/// Streaming transcript event (for real-time STT backends).
#[derive(Debug, Clone)]
pub enum TranscriptEvent {
    /// Partial/incremental transcript (updated as user speaks).
    Delta { item_id: String, text: String },
    /// Final transcript for a completed speech turn.
    Final {
        item_id: String,
        text: String,
        /// Who spoke, when diarization is on (local backends only).
        speaker: Option<SpeakerId>,
        /// Where the speech sat in the audio fed in, seconds `(start, end)` (local backends
        /// that cut their own segments); the clock the saved recording shares.
        span: Option<(f32, f32)>,
        /// The segment's raw speaker embedding, when diarization computed one.
        embedding: Option<Vec<f32>>,
    },
    /// Provider error.
    Error(String),
}

/// Audio input commands for streaming STT sessions.
#[derive(Debug)]
pub enum SttInput {
    /// Raw PCM audio samples (24kHz mono i16).
    Audio(Vec<i16>),
    /// Update the context prompt mid-session.
    UpdatePrompt(String),
    /// Gracefully close the connection.
    Close,
}

/// Errors from streaming STT operations.
#[derive(Debug, thiserror::Error)]
pub enum SttStreamError {
    /// Connection failed.
    #[error("Connection: {0}")]
    Connection(String),
    /// Protocol/transport error.
    #[error("Transport: {0}")]
    Transport(String),
    /// Channel closed unexpectedly.
    #[error("Channel closed")]
    ChannelClosed,
    /// Backend-specific error.
    #[error("{0}")]
    Other(String),
}

/// Streaming speech-to-text backend.
///
/// Implementations connect to an STT service and return a channel pair:
/// send `SttInput` commands, receive `TranscriptEvent` results.
#[allow(clippy::double_must_use)]
#[async_trait::async_trait]
pub trait StreamingSttBackend: Send + Sync {
    /// Connect to the streaming STT service.
    ///
    /// Returns `(audio_sender, transcript_receiver)`:
    /// - Send `SttInput::Audio(samples)` to stream audio
    /// - Send `SttInput::Close` to shut down
    /// - Receive `TranscriptEvent` for partial/final transcripts
    async fn connect(
        &self,
    ) -> Result<(mpsc::Sender<SttInput>, mpsc::Receiver<TranscriptEvent>), SttStreamError>;

    /// Backend name for display in status bar.
    fn display_name(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaker_labels() {
        assert_eq!(SpeakerId::Agent.label("Алина"), "Алина");
        assert_eq!(SpeakerId::Cluster(2).label("x"), "S2");
        assert_eq!(SpeakerId::Named("Ann".into()).label("x"), "Ann");
    }

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
    fn stt_stream_error_display() {
        let e = SttStreamError::Connection("timeout".into());
        assert_eq!(format!("{e}"), "Connection: timeout");

        let e = SttStreamError::ChannelClosed;
        assert_eq!(format!("{e}"), "Channel closed");
    }

    #[test]
    fn stt_input_variants() {
        let audio = SttInput::Audio(vec![100, -200]);
        let prompt = SttInput::UpdatePrompt("test".into());
        let close = SttInput::Close;
        assert!(matches!(audio, SttInput::Audio(_)));
        assert!(matches!(prompt, SttInput::UpdatePrompt(_)));
        assert!(matches!(close, SttInput::Close));
    }

    #[test]
    fn transcript_event_variants() {
        let delta = TranscriptEvent::Delta {
            item_id: "x".into(),
            text: "hel".into(),
        };
        let final_ = TranscriptEvent::Final {
            speaker: None,
            span: None,
            embedding: None,
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
