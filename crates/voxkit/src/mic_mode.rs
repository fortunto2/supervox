//! macOS microphone mode detection.
//!
//! Detects the active microphone mode (Standard, Voice Isolation, Wide Spectrum)
//! on macOS to auto-tune VAD parameters.
//!
//! Only available on macOS (`#[cfg(target_os = "macos")]`).

use crate::vad::VadConfig;

/// macOS microphone mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicMode {
    /// Standard mode — no Apple processing.
    Standard,
    /// Voice Isolation — Apple's ML-based noise cancellation.
    VoiceIsolation,
    /// Wide Spectrum — captures all audio including ambient sounds.
    WideSpectrum,
    /// Could not determine mode (e.g., non-macOS, detection failed).
    Unknown,
}

impl MicMode {
    /// Detect the current macOS microphone mode.
    ///
    /// Uses `system_profiler` and audio system queries to determine
    /// the active mic processing mode.
    pub fn detect() -> Self {
        detect_mic_mode_impl()
    }

    /// Get a VAD config tuned for this microphone mode.
    pub fn vad_config(&self) -> VadConfig {
        match self {
            MicMode::VoiceIsolation => VadConfig::voice_isolated(),
            MicMode::WideSpectrum | MicMode::Standard => VadConfig::default(),
            MicMode::Unknown => VadConfig::default(),
        }
    }

    /// Whether the mode applies Apple's Voice Isolation processing.
    pub fn is_voice_isolated(&self) -> bool {
        matches!(self, MicMode::VoiceIsolation)
    }

    /// Human-readable name.
    pub fn as_str(&self) -> &str {
        match self {
            MicMode::Standard => "Standard",
            MicMode::VoiceIsolation => "Voice Isolation",
            MicMode::WideSpectrum => "Wide Spectrum",
            MicMode::Unknown => "Unknown",
        }
    }
}

impl std::fmt::Display for MicMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Search paths for the mic-mode detection helper binary.
const BINARY_CANDIDATES: &[&str] = &["mic-mode-detect"];

/// Internal detection implementation.
///
/// Strategy:
/// 1. Try companion binary `mic-mode-detect` (fastest, most reliable)
/// 2. Fall back to parsing `system_profiler SPAudioDataType` output
/// 3. Default to Unknown
fn detect_mic_mode_impl() -> MicMode {
    // Try companion binary first
    if let Some(mode) = try_companion_binary() {
        return mode;
    }

    // Try system_profiler fallback
    if let Some(mode) = try_system_profiler() {
        return mode;
    }

    MicMode::Unknown
}

/// Try detecting via companion binary.
fn try_companion_binary() -> Option<MicMode> {
    for candidate in BINARY_CANDIDATES {
        // Check if binary exists in PATH or next to exe
        let paths_to_try = {
            let mut paths = vec![candidate.to_string()];
            if let Ok(exe) = std::env::current_exe()
                && let Some(dir) = exe.parent()
            {
                paths.push(dir.join(candidate).to_string_lossy().to_string());
            }
            paths
        };

        for path in &paths_to_try {
            if let Ok(output) = std::process::Command::new(path).output()
                && output.status.success()
            {
                let stdout = String::from_utf8_lossy(&output.stdout);
                return parse_mode_string(stdout.trim());
            }
        }
    }
    None
}

/// Try detecting via system_profiler.
fn try_system_profiler() -> Option<MicMode> {
    let output = std::process::Command::new("system_profiler")
        .arg("SPAudioDataType")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Look for mic mode indicators in the output
    if stdout.contains("Voice Isolation") {
        return Some(MicMode::VoiceIsolation);
    }
    if stdout.contains("Wide Spectrum") {
        return Some(MicMode::WideSpectrum);
    }

    // If we found audio info but no special mode, it's Standard
    if stdout.contains("Input") || stdout.contains("Microphone") {
        return Some(MicMode::Standard);
    }

    None
}

/// Parse mode string from companion binary output.
fn parse_mode_string(s: &str) -> Option<MicMode> {
    match s.to_lowercase().as_str() {
        "standard" => Some(MicMode::Standard),
        "voice_isolation" | "voice-isolation" | "voiceisolation" => Some(MicMode::VoiceIsolation),
        "wide_spectrum" | "wide-spectrum" | "widespectrum" => Some(MicMode::WideSpectrum),
        "unknown" => Some(MicMode::Unknown),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mic_mode_display() {
        assert_eq!(MicMode::Standard.to_string(), "Standard");
        assert_eq!(MicMode::VoiceIsolation.to_string(), "Voice Isolation");
        assert_eq!(MicMode::WideSpectrum.to_string(), "Wide Spectrum");
        assert_eq!(MicMode::Unknown.to_string(), "Unknown");
    }

    #[test]
    fn mic_mode_vad_config() {
        let vi = MicMode::VoiceIsolation.vad_config();
        assert_eq!(vi.speech_threshold, 0.35); // voice_isolated preset

        let std = MicMode::Standard.vad_config();
        assert_eq!(std.speech_threshold, 0.5); // default preset
    }

    #[test]
    fn mic_mode_is_voice_isolated() {
        assert!(MicMode::VoiceIsolation.is_voice_isolated());
        assert!(!MicMode::Standard.is_voice_isolated());
        assert!(!MicMode::WideSpectrum.is_voice_isolated());
        assert!(!MicMode::Unknown.is_voice_isolated());
    }

    #[test]
    fn parse_mode_strings() {
        assert_eq!(parse_mode_string("standard"), Some(MicMode::Standard));
        assert_eq!(
            parse_mode_string("voice_isolation"),
            Some(MicMode::VoiceIsolation)
        );
        assert_eq!(
            parse_mode_string("voice-isolation"),
            Some(MicMode::VoiceIsolation)
        );
        assert_eq!(
            parse_mode_string("wide_spectrum"),
            Some(MicMode::WideSpectrum)
        );
        assert_eq!(parse_mode_string("unknown"), Some(MicMode::Unknown));
        assert_eq!(parse_mode_string("garbage"), None);
    }

    #[test]
    fn detect_runs() {
        // Just verify detection doesn't panic
        let mode = MicMode::detect();
        // On CI or non-macOS, this will likely be Unknown or Standard
        assert!(matches!(
            mode,
            MicMode::Standard | MicMode::VoiceIsolation | MicMode::WideSpectrum | MicMode::Unknown
        ));
    }
}
