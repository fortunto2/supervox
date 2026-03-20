//! OpenAI Realtime API streaming STT via WebSocket.
//!
//! Connects to the OpenAI Realtime API, sends audio chunks, and receives
//! streaming transcript events (delta + final).

use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;

use crate::stt::{StreamingSttBackend, SttInput, SttStreamError, TranscriptEvent};

/// Default OpenAI Realtime API WebSocket URL.
const DEFAULT_REALTIME_URL: &str = "wss://api.openai.com/v1/realtime";

/// Configuration for streaming STT.
#[derive(Debug, Clone)]
pub struct StreamingSttConfig {
    /// OpenAI API key.
    pub api_key: String,
    /// Model identifier (e.g., "gpt-4o-mini-realtime-preview").
    pub model: String,
    /// Language hint (ISO 639-1).
    pub language: String,
    /// Vocabulary prompt (helps with domain-specific terms).
    pub prompt: Option<String>,
    /// VAD threshold for server-side turn detection.
    pub vad_threshold: f32,
    /// Silence duration before server ends turn (ms).
    pub silence_duration_ms: u32,
    /// Enable server-side noise reduction.
    pub noise_reduction: bool,
    /// Custom WebSocket URL (for proxies, self-hosted).
    pub url: Option<String>,
}

impl StreamingSttConfig {
    /// Create with API key and sensible defaults.
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            model: "gpt-4o-mini-realtime-preview".to_string(),
            language: "en".to_string(),
            prompt: None,
            vad_threshold: 0.5,
            silence_duration_ms: 700,
            noise_reduction: true,
            url: None,
        }
    }

    /// Set language hint.
    pub fn with_language(mut self, lang: &str) -> Self {
        self.language = lang.to_string();
        self
    }

    /// Set vocabulary prompt.
    pub fn with_prompt(mut self, prompt: &str) -> Self {
        self.prompt = Some(prompt.to_string());
        self
    }
}

/// Errors from streaming STT operations.
#[derive(Debug, Error)]
pub enum WsError {
    /// WebSocket connection failed.
    #[error("WebSocket connection: {0}")]
    Connection(String),
    /// WebSocket message error.
    #[error("WebSocket: {0}")]
    WebSocket(String),
    /// JSON serialization/deserialization error.
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// Channel closed unexpectedly.
    #[error("Channel closed")]
    ChannelClosed,
}

impl From<tungstenite::Error> for WsError {
    fn from(e: tungstenite::Error) -> Self {
        WsError::WebSocket(e.to_string())
    }
}

/// Streaming STT session using OpenAI Realtime API.
pub struct OpenAiStreamingStt {
    config: StreamingSttConfig,
}

impl OpenAiStreamingStt {
    /// Create a new OpenAI streaming STT backend.
    pub fn new(config: StreamingSttConfig) -> Self {
        Self { config }
    }

    /// Connect to OpenAI Realtime API (static method for backward compatibility).
    pub async fn connect_static(
        config: StreamingSttConfig,
    ) -> Result<(mpsc::Sender<SttInput>, mpsc::Receiver<TranscriptEvent>), WsError> {
        Self::do_connect(&config)
            .await
            .map_err(|e| WsError::Connection(e.to_string()))
    }

    /// Internal connect logic using local error type.
    async fn do_connect(
        config: &StreamingSttConfig,
    ) -> Result<(mpsc::Sender<SttInput>, mpsc::Receiver<TranscriptEvent>), WsError> {
        let url = build_ws_url(config);

        let request = build_ws_request(&url, &config.api_key)?;

        let (ws_stream, _response) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|e| WsError::Connection(e.to_string()))?;

        let (ws_writer, ws_reader) = ws_stream.split();

        let (input_tx, input_rx) = mpsc::channel::<SttInput>(64);
        let (transcript_tx, transcript_rx) = mpsc::channel::<TranscriptEvent>(64);

        // Send session config
        let session_config = build_session_config(config);

        // Writer task: reads from input_rx, sends to WebSocket
        let tx_clone = transcript_tx.clone();
        tokio::spawn(writer_task(ws_writer, input_rx, session_config, tx_clone));

        // Reader task: reads from WebSocket, sends to transcript_tx
        tokio::spawn(reader_task(ws_reader, transcript_tx));

        Ok((input_tx, transcript_rx))
    }
}

#[async_trait::async_trait]
impl StreamingSttBackend for OpenAiStreamingStt {
    async fn connect(
        &self,
    ) -> Result<(mpsc::Sender<SttInput>, mpsc::Receiver<TranscriptEvent>), SttStreamError> {
        Self::do_connect(&self.config)
            .await
            .map_err(|e| SttStreamError::Connection(e.to_string()))
    }

    fn display_name(&self) -> &str {
        "realtime"
    }
}

/// Build the WebSocket URL with model query parameter.
fn build_ws_url(config: &StreamingSttConfig) -> String {
    let base = config.url.as_deref().unwrap_or(DEFAULT_REALTIME_URL);
    format!("{}?model={}", base, config.model)
}

/// Build the HTTP upgrade request with auth headers.
fn build_ws_request(url: &str, api_key: &str) -> Result<http::Request<()>, WsError> {
    let uri: http::Uri = url
        .parse()
        .map_err(|e: http::uri::InvalidUri| WsError::Connection(e.to_string()))?;

    let host = uri.host().unwrap_or("api.openai.com").to_owned();

    http::Request::builder()
        .uri(uri)
        .header("Host", host)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("OpenAI-Beta", "realtime=v1")
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tungstenite::handshake::client::generate_key(),
        )
        .body(())
        .map_err(|e| WsError::Connection(e.to_string()))
}

/// Build session.update config message for the Realtime API.
fn build_session_config(config: &StreamingSttConfig) -> serde_json::Value {
    let mut session = serde_json::json!({
        "type": "session.update",
        "session": {
            "modalities": ["text"],
            "input_audio_format": "pcm16",
            "input_audio_transcription": {
                "model": "gpt-4o-mini-transcribe",
                "language": config.language,
            },
            "turn_detection": {
                "type": "server_vad",
                "threshold": config.vad_threshold,
                "silence_duration_ms": config.silence_duration_ms,
            },
        }
    });

    if let Some(ref prompt) = config.prompt {
        session["session"]["instructions"] = serde_json::Value::String(prompt.clone());
    }

    if config.noise_reduction {
        session["session"]["input_audio_noise_reduction"] =
            serde_json::json!({"type": "near_field"});
    }

    session
}

/// Encode audio samples as a Realtime API `input_audio_buffer.append` event.
fn encode_audio_event(samples: &[i16]) -> serde_json::Value {
    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    serde_json::json!({
        "type": "input_audio_buffer.append",
        "audio": encoded,
    })
}

/// Parse a transcript-related event from the Realtime API.
fn parse_transcript_event(msg: &serde_json::Value) -> Option<TranscriptEvent> {
    let event_type = msg["type"].as_str()?;

    match event_type {
        "conversation.item.input_audio_transcription.delta" => {
            let item_id = msg["item_id"].as_str().unwrap_or("").to_string();
            let text = msg["delta"].as_str().unwrap_or("").to_string();
            if text.is_empty() {
                return None;
            }
            Some(TranscriptEvent::Delta { item_id, text })
        }
        "conversation.item.input_audio_transcription.completed" => {
            let item_id = msg["item_id"].as_str().unwrap_or("").to_string();
            let text = msg["transcript"].as_str().unwrap_or("").to_string();
            Some(TranscriptEvent::Final { item_id, text })
        }
        "error" => {
            let error_msg = msg["error"]["message"]
                .as_str()
                .unwrap_or("Unknown error")
                .to_string();
            Some(TranscriptEvent::Error(error_msg))
        }
        _ => None,
    }
}

/// Resample f32 audio to 24kHz i16 (OpenAI Realtime API format).
pub fn resample_to_24k(samples: &[f32], src_rate: u32) -> Vec<i16> {
    crate::types::resample_to_24k_i16(samples, src_rate)
}

/// Writer task: forwards SttInput commands to the WebSocket.
async fn writer_task<S>(
    mut ws_writer: S,
    mut input_rx: mpsc::Receiver<SttInput>,
    session_config: serde_json::Value,
    error_tx: mpsc::Sender<TranscriptEvent>,
) where
    S: futures_util::Sink<tungstenite::Message, Error = tungstenite::Error> + Unpin,
{
    // Send session config first
    let config_msg = tungstenite::Message::Text(session_config.to_string().into());
    if let Err(e) = ws_writer.send(config_msg).await {
        tracing::error!("Failed to send session config: {e}");
        let _ = error_tx
            .send(TranscriptEvent::Error(format!("Session config: {e}")))
            .await;
        return;
    }

    while let Some(input) = input_rx.recv().await {
        let result = match input {
            SttInput::Audio(samples) => {
                let event = encode_audio_event(&samples);
                let msg = tungstenite::Message::Text(event.to_string().into());
                ws_writer.send(msg).await
            }
            SttInput::UpdatePrompt(prompt) => {
                let update = serde_json::json!({
                    "type": "session.update",
                    "session": {
                        "instructions": prompt,
                    }
                });
                let msg = tungstenite::Message::Text(update.to_string().into());
                ws_writer.send(msg).await
            }
            SttInput::Close => {
                let _ = ws_writer.close().await;
                break;
            }
        };

        if let Err(e) = result {
            tracing::warn!("WebSocket write error: {e}");
            break;
        }
    }
}

/// Reader task: reads WebSocket messages and forwards transcript events.
async fn reader_task<S>(mut ws_reader: S, transcript_tx: mpsc::Sender<TranscriptEvent>)
where
    S: futures_util::Stream<Item = Result<tungstenite::Message, tungstenite::Error>> + Unpin,
{
    while let Some(msg_result) = ws_reader.next().await {
        let msg = match msg_result {
            Ok(tungstenite::Message::Text(text)) => text,
            Ok(tungstenite::Message::Close(_)) => break,
            Ok(_) => continue, // Ignore binary, ping, pong
            Err(e) => {
                tracing::warn!("WebSocket read error: {e}");
                let _ = transcript_tx
                    .send(TranscriptEvent::Error(e.to_string()))
                    .await;
                break;
            }
        };

        let parsed: serde_json::Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!("Non-JSON WebSocket message: {e}");
                continue;
            }
        };

        if let Some(event) = parse_transcript_event(&parsed)
            && transcript_tx.send(event).await.is_err()
        {
            break; // Receiver dropped
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let config = StreamingSttConfig::new("test-key");
        assert_eq!(config.model, "gpt-4o-mini-realtime-preview");
        assert_eq!(config.language, "en");
        assert_eq!(config.vad_threshold, 0.5);
        assert_eq!(config.silence_duration_ms, 700);
        assert!(config.noise_reduction);
        assert!(config.url.is_none());
    }

    #[test]
    fn config_builder() {
        let config = StreamingSttConfig::new("key")
            .with_language("ru")
            .with_prompt("tech meeting");
        assert_eq!(config.language, "ru");
        assert_eq!(config.prompt.as_deref(), Some("tech meeting"));
    }

    #[test]
    fn encode_audio_event_format() {
        let samples: Vec<i16> = vec![100, -200, 300];
        let event = encode_audio_event(&samples);
        assert_eq!(event["type"], "input_audio_buffer.append");
        assert!(event["audio"].is_string());
        // Verify base64 roundtrip
        let encoded = event["audio"].as_str().unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        assert_eq!(decoded.len(), 6); // 3 i16 * 2 bytes
    }

    #[test]
    fn parse_delta_event() {
        let msg = serde_json::json!({
            "type": "conversation.item.input_audio_transcription.delta",
            "item_id": "item_123",
            "delta": "Hello"
        });
        let event = parse_transcript_event(&msg).unwrap();
        match event {
            TranscriptEvent::Delta { item_id, text } => {
                assert_eq!(item_id, "item_123");
                assert_eq!(text, "Hello");
            }
            _ => panic!("Expected Delta event"),
        }
    }

    #[test]
    fn parse_final_event() {
        let msg = serde_json::json!({
            "type": "conversation.item.input_audio_transcription.completed",
            "item_id": "item_456",
            "transcript": "Hello, world!"
        });
        let event = parse_transcript_event(&msg).unwrap();
        match event {
            TranscriptEvent::Final { item_id, text } => {
                assert_eq!(item_id, "item_456");
                assert_eq!(text, "Hello, world!");
            }
            _ => panic!("Expected Final event"),
        }
    }

    #[test]
    fn parse_error_event() {
        let msg = serde_json::json!({
            "type": "error",
            "error": {
                "message": "Rate limit exceeded"
            }
        });
        let event = parse_transcript_event(&msg).unwrap();
        match event {
            TranscriptEvent::Error(text) => {
                assert_eq!(text, "Rate limit exceeded");
            }
            _ => panic!("Expected Error event"),
        }
    }

    #[test]
    fn parse_unknown_event() {
        let msg = serde_json::json!({
            "type": "session.created",
            "session": {}
        });
        assert!(parse_transcript_event(&msg).is_none());
    }

    #[test]
    fn session_config_structure() {
        let config = StreamingSttConfig::new("key");
        let session = build_session_config(&config);
        assert_eq!(session["type"], "session.update");
        assert_eq!(session["session"]["input_audio_format"], "pcm16");
        assert_eq!(session["session"]["turn_detection"]["type"], "server_vad");
        // Noise reduction enabled by default
        assert!(session["session"]["input_audio_noise_reduction"].is_object());
    }
}
