//! # voxkit — Voice pipeline toolkit
//!
//! Provides abstractions for building voice-powered apps in Rust:
//! - **Audio types** — `AudioChunk`, `Transcript`, `Segment`
//! - **STT** — Speech-to-text trait + OpenAI backend
//! - **VAD** — Voice activity detection trait + Silero backend
//! - **TTS** — Text-to-speech trait + OpenAI backend
//! - **Mic** — Cross-platform microphone capture via cpal
//! - **System audio** — macOS system audio capture via ScreenCaptureKit
//! - **Mic mode** — macOS microphone mode detection
//! - **TTS player** — Background TTS playback with sentence splitting
//!
//! All backends are feature-gated. Core types have zero dependencies.

pub mod types;

pub mod stt;
pub mod vad;

#[cfg(feature = "openai")]
pub mod openai_stt;

#[cfg(feature = "silero")]
pub mod silero;

#[cfg(feature = "realtime")]
pub mod realtime_stt;

#[cfg(feature = "openai-tts")]
pub mod tts;

#[cfg(feature = "player")]
pub mod tts_player;

#[cfg(feature = "mic")]
pub mod mic;

#[cfg(all(target_os = "macos", feature = "macos-system-audio"))]
pub mod system_audio;

#[cfg(all(target_os = "macos", feature = "macos-mic-mode"))]
pub mod mic_mode;

// Re-exports
pub use types::{AudioChunk, Segment, Speaker, Transcript};

pub use stt::{StreamingSttBackend, SttBackend, SttError, SttInput, SttStreamError};
pub use vad::{VadBackend, VadConfig, VadEvent};
