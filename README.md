# SuperVox

Voice-powered productivity TUI. Live call assistant + post-call analysis + agent chat.

## Modes

- **Live** -- real-time subtitles, translation, and rolling summary during calls
- **Analysis** -- post-call summary, action items, follow-up draft
- **Agent** -- chat with call history, search across past calls

## Quick Start

```bash
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

```toml
# ~/.supervox/config.toml
[general]
my_language = "ru"

[live]
stt_backend = "realtime"
summary_lag_secs = 5
capture = "mic+system"

[analysis]
llm_model = "gemini-2.5-flash"
follow_up_language = "auto"
```

## License

MIT
