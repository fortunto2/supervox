# SuperVox

Voice-powered productivity TUI. Live call assistant + post-call analysis + agent chat.

## Modes

- **Live** -- real-time subtitles, translation, and rolling summary during calls
- **Analysis** -- post-call summary, action items, follow-up draft
- **Agent** -- chat with call history, search across past calls

## Prerequisites

- Rust 2024 edition
- `OPENAI_API_KEY` environment variable (for realtime STT)
- macOS for system audio capture (`system-audio-tap` binary)

## Quick Start

```bash
export OPENAI_API_KEY="sk-..."
make test      # run all tests
make check     # test + clippy + fmt
make run       # launch TUI
make install   # install to ~/.cargo/bin
```

## Usage

```bash
supervox live                    # live call assistant
supervox analyze <call.json>     # post-call analysis
supervox agent                   # chat with history
supervox calls                   # list past calls
```

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Voice pipeline | voxkit (STT, VAD, TTS, mic, system audio) |
| LLM agent | sgr-agent (tool calling, sessions, compaction) |
| TUI | ratatui + sgr-agent-tui |
| Real-time STT | OpenAI Realtime WebSocket |
| LLM | Gemini Flash / OpenRouter / Ollama |

## Config

Config is loaded from `~/.supervox/config.toml` at startup. A default is created if missing.

```toml
# ~/.supervox/config.toml
my_language = "ru"              # target language for translation/summary
stt_backend = "realtime"        # "realtime" (WebSocket) | "openai" (batch)
llm_model = "gemini-2.5-flash"  # model for translation + summary
summary_lag_secs = 5            # rolling summary interval
capture = "mic+system"          # "mic" | "mic+system"
```

### System audio setup (macOS)

System audio capture uses ScreenCaptureKit via the `system-audio-tap` helper binary.
If unavailable, SuperVox falls back to mic-only mode automatically.

## License

MIT
