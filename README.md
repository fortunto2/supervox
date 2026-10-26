# SuperVox

Voice-powered productivity TUI. Live call assistant + post-call analysis + agent chat.

## Modes

- **Live** -- real-time subtitles, translation, rolling summary, and audio recording
- **Analysis** -- post-call summary, action items, follow-up draft
- **Agent** -- chat with call history, search across past calls
- **History** -- browse past calls, open any call in Analysis mode
- **Sessions** -- every recording in one list: fuzzy by name/date/title, or a phrase across all transcripts; read any of them

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
supervox sessions                    # browse every recording (sessions + calls), fuzzy or phrase search
supervox sessions --list [--json]    # print them, newest first
supervox sessions --grep "phrase"    # every line with the phrase, with t=m:ss (--json too)

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
| `Enter` / `b` | Bookmark (while recording) |
| `h` | Open call history (when idle) |
| `m` | Next microphone (when idle) |
| `Tab` / `Shift+Tab` | Focus the Transcript or the Summary pane (thick border) |
| `↑/↓`, `j/k` | Scroll the focused pane by a line |
| `PgUp/PgDn` (`Ctrl+B/F`) | Scroll by a page |
| `Home` / `g` | Top; the pane stops following new lines |
| `End` / `G` | Bottom; follow new lines again |
| Mouse wheel | Scroll the pane under the pointer (and focus it) |
| `/` or `o` | Sessions browser (recording carries on behind it) |
| `q` | Quit (when idle) |

The transcript follows the newest line until you scroll up; its title then says
`42% · End follows`, and new lines wait below until `End` (or scrolling back to the bottom).
The summary is a document: it starts at the top and keeps its place when the agent rewrites
`summary.md`; `End` on it follows its tail instead.

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

### Sessions browser

`supervox sessions`, or `/` / `o` from Live. Lists every recording: session folders
(`live/sessions/<stamp>[-slug]/`, including an agent's `stream-part*.jsonl`), the older flat
`sessions/<ts>.jsonl` + `<ts>.summary.md`, saved calls in `calls/`, and a `current.jsonl` an
older supervox is still writing (marked `●`). A call hard-linked to a session is listed once.
Discovery reads only metadata, summary headings and the last 32 KB of each stream (never a
WAV); transcripts load on a background thread, newest first, so phrase search covers what
is loaded and the footer counts the rest.

| Key | Action |
|-----|--------|
| type | Filter (fuzzy: name, date, summary title) or phrase |
| `Tab` | Switch between fuzzy and phrase search |
| `↑/↓`, `PgUp/PgDn`, `Home/End` | Select |
| `Enter` | Read: transcript and summary side by side |
| `Ctrl+U` / `Ctrl+W` | Clear the query / a word |
| `Esc` | Back (or quit, when started as `supervox sessions`) |

In the reader: `Tab` or `←/→` switches pane, `↑/↓ j/k PgUp/PgDn Home/End` and the wheel
scroll it, `n` / `N` jump to the next / previous match of the phrase (marked yellow, the
current one on a dark line), `Esc` / `q` / `/` go back to the list.

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

Each recording lives in one folder from its first second,
`~/.supervox/live/sessions/<YYYYMMDD-HHMMSS of its start>/`: `stream.jsonl`,
`summary.md`, `summaries/<HHMMSS>.md` (every version the panel showed) and, after stop,
`recording.wav` (a symlink to the call's WAV in `calls/`). `live/current.jsonl` and
`live/summary.md` are symlinks into that folder, so agents keep the fixed names. An editor
that saves by rename replaces the `summary.md` link with a plain file; supervox never writes
to that path, it copies each new version into the folder and relinks on the next start.
A recording with no speech is removed on stop, or renamed `<stamp>-empty/` if an agent left
files in it. An older supervox's flat `current.jsonl` is archived the old way on the next
start; earlier still, sessions were flat `sessions/<ts>.jsonl`.

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
