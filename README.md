# SuperVox

Voice-powered productivity TUI. Live call assistant + post-call analysis + agent chat.

## Modes

- **Live** -- real-time subtitles, translation, rolling summary, and audio recording
- **Analysis** -- post-call summary, action items, follow-up draft
- **Agent** -- chat with call history, search across past calls
- **History** -- browse past calls, open any call in Analysis mode

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
supervox live                        # live call assistant
supervox analyze <call.json>         # post-call analysis
supervox analyze <call.json> --json  # output analysis as JSON
supervox agent                       # chat with history
supervox calls                       # list past calls
supervox calls --json                # output calls as JSON
supervox delete <call-id>            # delete a call (with confirmation)
supervox delete <call-id> --force    # delete without confirmation
supervox export <call-id>            # export call as markdown to stdout
supervox export <call-id> -o file.md # export to file
supervox search <query>              # search call transcripts
supervox search <query> --json       # output matches as JSON
supervox play <call-id>              # play audio recording in system player

# Use local Ollama instead of cloud LLM
supervox --local live
```

### Global keybindings

| Key | Action |
|-----|--------|
| `?` | Show help overlay with all keybindings for current mode |
| `Ctrl+C` | Quit immediately |

### Live mode

| Key | Action |
|-----|--------|
| `r` | Start recording |
| `s` | Stop recording |
| `h` | Open call history (when idle) |
| `m` | Next microphone (when idle) |
| `q` | Quit (when idle) |

Speaker labels are color-coded: **You** (cyan) and **Them** (yellow).

Audio is automatically saved as WAV (16-bit PCM mono) alongside the call JSON in `~/.supervox/calls/`. Use `supervox play <call-id>` or press `p` in Analysis mode to listen back.

### Analysis mode

Opens a call JSON file, runs LLM analysis automatically (summary, action items, mood, themes).

| Key | Action |
|-----|--------|
| `f` | Generate follow-up email |
| `c` | Copy analysis to clipboard |
| `C` | Copy follow-up to clipboard |
| `e` | Export call + analysis as markdown to clipboard |
| `p` | Play audio recording (if available) |
| `h` | Open call history |
| Arrow keys | Scroll |
| `q` | Quit |

### History mode

Browse and manage past calls.

| Key | Action |
|-----|--------|
| `↑/↓/j/k` | Navigate |
| `Enter` | Open in Analysis |
| `d` | Delete call (y/n confirmation) |
| `Esc` | Back to previous mode |
| `q` | Quit |

### Agent mode

Chat with your call history. The agent loads the last 10 calls as context and streams LLM responses in real-time.

| Key | Action |
|-----|--------|
| Type + Enter | Send question |
| Esc | Quit |

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
my_language = "ru"              # summary / translation language (ISO code)
stt_backend = "parakeet"        # "realtime" (OpenAI WS) | "whisper" | "parakeet" (local, English)
capture = "mic"                 # "mic" | "mic+system"
mic_device = "MacBook Pro Microphone"   # omit for the system default; `m` in Live cycles devices and saves the choice here

# Local STT segmentation + speakers (parakeet)
vad_pause_ms = 1200             # trailing silence that closes a phrase
diarize = true                  # label mic speakers S1..Sn (needs the speaker model, see below)
max_speakers = 3

# Summary panel
summary = "external"            # "external" (agent writes summary.md) | "internal" (LLM below) | "off"
summary_lag_secs = 30
llm_backend = "ollama"          # "auto" | "ollama"
ollama_model = "gemma3:4b"
llm_model = "gemini-2.5-flash"
translate = false
```

### Live stream for agents

While recording, every final transcript line is appended to
`~/.supervox/live/current.jsonl` (`{"type":"transcript","t":83,"speaker":"S1","text":"…"}`,
plus `start`/`summary`/`stop` events). With `summary = "external"` the right panel
shows whatever an outside agent writes to `~/.supervox/live/summary.md`, re-read every
2 s. This is how Claude Code (or any script) summarizes, researches and critiques a
talk for you while supervox only listens.

The next recording archives the session into one folder,
`~/.supervox/live/sessions/<YYYYMMDD-HHMMSS of its start>/`: `stream.jsonl`,
`summary.md` (only if written after the stream started), `summaries/<HHMMSS>.md` (every
version the panel showed) and `recording.wav` (a symlink to the call's WAV in `calls/`).
A stream with no speech gets the suffix `-empty`; a folder an agent already made as
`<stamp>-<slug>/` is reused. Sessions archived before this are flat `sessions/<ts>.jsonl`.

### Speaker labels (diarization)

Local STT can tell 1–3 mic speakers apart with a WeSpeaker CAM++ embedding model
(29 MB, one embedding per phrase, online cosine clustering):

```bash
curl -L -o ~/.supervox/models/wespeaker_en_voxceleb_CAM++.onnx \
  https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/wespeaker_en_voxceleb_CAM++.onnx
```

Good for "same person kept talking"; it is not identity verification.


### System audio setup (macOS)

System audio capture uses ScreenCaptureKit via the `system-audio-tap` helper binary.
If unavailable, SuperVox falls back to mic-only mode automatically.

## License

MIT
