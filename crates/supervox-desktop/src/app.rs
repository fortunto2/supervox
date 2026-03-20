//! SuperVox Desktop — Dioxus app component.

use dioxus::prelude::*;
use std::time::Instant;

use crate::audio::{AudioEvent, AudioPipeline};

#[derive(Clone)]
struct AppState {
    is_recording: bool,
    mic_level: f32,
    lines: Vec<String>,
    current_delta: Option<String>,
    summary: String,
    timer_start: Option<Instant>,
    stt_backend: String,
    error: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        let config =
            supervox_agent::storage::load_config(&supervox_agent::storage::default_config_path())
                .unwrap_or_default();
        let stt = match std::env::var("SUPERVOX_STT_BACKEND").ok().as_deref() {
            Some(s) => s.to_string(),
            None => config.stt_backend.to_string(),
        };
        Self {
            is_recording: false,
            mic_level: 0.0,
            lines: Vec::new(),
            current_delta: None,
            summary: String::new(),
            timer_start: None,
            stt_backend: stt,
            error: None,
        }
    }
}

fn toggle_recording(
    state: &mut Signal<AppState>,
    pipeline: &mut Signal<Option<AudioPipeline>>,
    event_rx: &mut Signal<Option<tokio::sync::mpsc::UnboundedReceiver<AudioEvent>>>,
) {
    let is_rec = state.read().is_recording;
    if is_rec {
        // Stop
        if let Some(ref mut p) = *pipeline.write() {
            p.stop();
        }
        let mut s = state.write();
        s.is_recording = false;
        s.mic_level = 0.0;
        s.timer_start = None;
    } else {
        // Start
        let config =
            supervox_agent::storage::load_config(&supervox_agent::storage::default_config_path())
                .unwrap_or_default();
        let mut p = AudioPipeline::new();
        match p.start(&config) {
            Ok(rx) => {
                *event_rx.write() = Some(rx);
                *pipeline.write() = Some(p);
                let mut s = state.write();
                s.is_recording = true;
                s.timer_start = Some(Instant::now());
                s.error = None;
            }
            Err(e) => {
                state.write().error = Some(e);
            }
        }
    }
}

#[component]
pub fn App() -> Element {
    let mut state = use_signal(AppState::default);
    let mut pipeline: Signal<Option<AudioPipeline>> = use_signal(|| None);
    let mut event_rx: Signal<Option<tokio::sync::mpsc::UnboundedReceiver<AudioEvent>>> =
        use_signal(|| None);

    // Poll audio events at 60fps
    use_future(move || async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(16)).await;
            let mut rx_guard = event_rx.write();
            if let Some(ref mut rx) = *rx_guard {
                while let Ok(event) = rx.try_recv() {
                    let mut s = state.write();
                    match event {
                        AudioEvent::MicLevel(level) => s.mic_level = level,
                        AudioEvent::Transcript { text, is_final } => {
                            if is_final {
                                s.lines.push(text);
                                s.current_delta = None;
                            } else {
                                s.current_delta = Some(text);
                            }
                        }
                        AudioEvent::Error(e) => s.error = Some(e),
                        AudioEvent::Stopped => {
                            s.is_recording = false;
                            s.mic_level = 0.0;
                            s.timer_start = None;
                        }
                    }
                }
            }
        }
    });

    let s = state.read();

    let elapsed = s
        .timer_start
        .map(|t| {
            let secs = t.elapsed().as_secs();
            format!("{}:{:02}", secs / 60, secs % 60)
        })
        .unwrap_or_else(|| "0:00".into());

    let vu_bars: Vec<f32> = (0..8)
        .map(|i| {
            let threshold = i as f32 * 0.15 / 8.0;
            if s.mic_level > threshold {
                ((s.mic_level - threshold) / (0.15 / 8.0)).min(1.0)
            } else {
                0.0
            }
        })
        .collect();

    let is_recording = s.is_recording;
    let lines = s.lines.clone();
    let current_delta = s.current_delta.clone();
    let stt_backend = s.stt_backend.clone();
    let error = s.error.clone();
    let summary = s.summary.clone();

    // Drop the read guard before rsx
    drop(s);

    rsx! {
        document::Link { rel: "stylesheet", href: asset!("/assets/style.css") }

        div {
            class: "app",
            tabindex: 0,
            autofocus: true,
            onkeydown: move |evt: KeyboardEvent| {
                match evt.key() {
                    Key::Character(c) if c == " " => {
                        toggle_recording(&mut state, &mut pipeline, &mut event_rx);
                    }
                    Key::Character(c) if c == "q" || c == "Q" => {
                        // Quit - stop recording first
                        if state.read().is_recording {
                            toggle_recording(&mut state, &mut pipeline, &mut event_rx);
                        }
                        std::process::exit(0);
                    }
                    _ => {}
                }
            },

            // Status bar
            div { class: "status-bar",
                div {
                    class: if is_recording { "rec-dot" } else { "rec-dot idle" },
                }
                if is_recording {
                    span { "REC" }
                } else {
                    span { "IDLE" }
                }

                span { "mic " }
                div { class: "vu-meter",
                    for (i, &level) in vu_bars.iter().enumerate() {
                        div {
                            key: "{i}",
                            class: "vu-bar",
                            style: "height: {(level * 14.0 + 2.0).min(16.0)}px",
                        }
                    }
                }

                span { class: "stt-label", "STT: {stt_backend}" }
                span { class: "timer", "{elapsed}" }

                if let Some(ref err) = error {
                    span { style: "color: var(--error); margin-left: auto;", "{err}" }
                }
            }

            // Main content
            div {
                class: "content",
                onclick: move |_| {
                    toggle_recording(&mut state, &mut pipeline, &mut event_rx);
                },
                style: "cursor: pointer;",

                // Transcript panel
                div { class: "transcript-panel",
                    for (i, line) in lines.iter().enumerate() {
                        div { key: "{i}", class: "line",
                            span { class: "speaker mic", "You: " }
                            span { class: "text", "{line}" }
                        }
                    }
                    if let Some(ref delta) = current_delta {
                        div { class: "line delta",
                            span { class: "speaker mic", "You: " }
                            span { class: "text", "{delta}" }
                        }
                    }
                    if lines.is_empty() && current_delta.is_none() && !is_recording {
                        div { style: "color: var(--text-dim); padding: 20px; font-size: 16px;",
                            "Click here or press Space to start recording"
                        }
                    }
                }

                // Summary panel
                div { class: "summary-panel",
                    h3 { "Rolling Summary" }
                    if summary.is_empty() {
                        p { style: "color: var(--text-dim);", "Summary will appear during recording..." }
                    } else {
                        p { "{summary}" }
                    }
                }
            }

            // Footer
            div { class: "footer",
                span { kbd { "Space" } " rec/stop" }
                span { kbd { "B" } " bookmark" }
                span { kbd { "Q" } " quit" }
            }
        }
    }
}
