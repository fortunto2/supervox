use anyhow::Result;
use clap::{Args, Parser, Subcommand};

mod agent_loop;
mod analysis_pipeline;
mod app;
mod audio;
mod clipboard;
mod help;
mod intelligence;
mod modes;

#[derive(Parser)]
#[command(name = "supervox", about = "Voice-powered productivity TUI", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Use local Ollama LLM instead of cloud model
    #[arg(long, global = true)]
    local: bool,
}

/// Shared filter flags for narrowing call lists by tag and/or date range.
#[derive(Args, Debug, Default)]
struct FilterArgs {
    /// Filter by tag (repeatable, OR logic)
    #[arg(long = "tag")]
    tags: Vec<String>,
    /// Filter calls from this date onward (YYYY-MM-DD)
    #[arg(long)]
    since: Option<String>,
    /// Filter calls up to this date (YYYY-MM-DD)
    #[arg(long)]
    until: Option<String>,
}

impl FilterArgs {
    fn to_call_filter(&self) -> Result<supervox_agent::types::CallFilter> {
        let since = self
            .since
            .as_deref()
            .map(|s| {
                chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                    .map_err(|e| anyhow::anyhow!("Invalid --since date \"{s}\": {e}"))
            })
            .transpose()?;
        let until = self
            .until
            .as_deref()
            .map(|s| {
                chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                    .map_err(|e| anyhow::anyhow!("Invalid --until date \"{s}\": {e}"))
            })
            .transpose()?;
        Ok(supervox_agent::types::CallFilter {
            tags: self.tags.clone(),
            since,
            until,
        })
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Live call assistant — real-time subtitles + translation + rolling summary
    Live,
    /// Post-call analysis — summary, action items, follow-up draft
    Analyze {
        /// Path to call JSON file
        file: String,
        /// Output as JSON (non-TUI)
        #[arg(long)]
        json: bool,
    },
    /// Agent chat — Q&A over call history
    Agent,
    /// List past calls
    Calls {
        /// Output as JSON
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        filter: FilterArgs,
    },
    /// Delete a call by ID
    Delete {
        /// Call ID (suffix match)
        call_id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Export a call as markdown
    Export {
        /// Call ID (suffix match)
        call_id: String,
        /// Write to file instead of stdout
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Search call transcripts
    Search {
        /// Search query
        query: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        filter: FilterArgs,
    },
    /// Cross-call insights — recurring themes, mood trends, action items
    Insights {
        /// Output as JSON
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        filter: FilterArgs,
    },
    /// Aggregate call statistics — total calls, duration, analysis coverage
    Stats {
        /// Output as JSON
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        filter: FilterArgs,
    },
    /// List all unique tags with counts
    Tags {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Batch-analyze all calls missing analysis
    #[command(name = "analyze-all")]
    AnalyzeAll {
        /// List unanalyzed calls without processing
        #[arg(long)]
        dry_run: bool,
    },
    /// Import an external audio file — transcribe, save as Call, optionally analyze
    Import {
        /// Path to audio file (WAV, MP3, M4A, FLAC, OGG, WebM)
        file: String,
        /// Skip auto-analysis after transcription
        #[arg(long)]
        no_analyze: bool,
        /// Output resulting Call as JSON
        #[arg(long)]
        json: bool,
        /// Override language (ISO 639-1, e.g. "en", "ru")
        #[arg(long)]
        language: Option<String>,
    },
    /// Play audio recording for a call
    Play {
        /// Call ID (suffix match)
        call_id: String,
    },
    /// List action items across calls
    Actions {
        /// Include completed actions
        #[arg(long)]
        all: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        filter: FilterArgs,
        #[command(subcommand)]
        action: Option<ActionCommands>,
    },
}

#[derive(Subcommand)]
enum ActionCommands {
    /// Mark an action item as complete
    Done {
        /// Action ID prefix
        id: String,
    },
    /// Undo action completion
    Undo {
        /// Action ID prefix
        id: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file (project dir or parents) — ignore if missing
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();

    // Apply --local override: switch to Ollama + Whisper backends
    if cli.local {
        // SAFETY: set_var before any threads are spawned (single-threaded at this point)
        unsafe {
            std::env::set_var("SUPERVOX_LLM_BACKEND", "ollama");
            std::env::set_var("SUPERVOX_STT_BACKEND", "whisper");
        };
        check_ollama_health();
    }

    match cli.command {
        Some(Commands::Calls { json, filter }) => {
            cmd_calls(json, &filter)?;
        }
        Some(Commands::Delete { call_id, force }) => {
            cmd_delete(&call_id, force)?;
        }
        Some(Commands::Export { call_id, output }) => {
            cmd_export(&call_id, output.as_deref())?;
        }
        Some(Commands::Search {
            query,
            json,
            filter,
        }) => {
            cmd_search(&query, json, &filter)?;
        }
        Some(Commands::Insights { json, filter }) => {
            cmd_insights(json, &filter).await?;
        }
        Some(Commands::Stats { json, filter }) => {
            cmd_stats(json, &filter)?;
        }
        Some(Commands::Tags { json }) => {
            cmd_tags(json)?;
        }
        Some(Commands::AnalyzeAll { dry_run }) => {
            cmd_analyze_all(dry_run).await?;
        }
        Some(Commands::Import {
            file,
            no_analyze,
            json,
            language,
        }) => {
            cmd_import(&file, no_analyze, json, language.as_deref()).await?;
        }
        Some(Commands::Play { call_id }) => {
            cmd_play(&call_id)?;
        }
        Some(Commands::Actions {
            all,
            json,
            filter,
            action,
        }) => match action {
            Some(ActionCommands::Done { id }) => cmd_action_done(&id)?,
            Some(ActionCommands::Undo { id }) => cmd_action_undo(&id)?,
            None => cmd_actions(all, json, &filter)?,
        },
        Some(Commands::Live) | None => {
            let config = supervox_agent::storage::load_config(
                &supervox_agent::storage::default_config_path(),
            )
            .unwrap_or_default();
            let stt = crate::audio::effective_stt_backend(&config);

            // Pre-flight: check API key for realtime STT
            if stt == supervox_agent::types::SttBackend::Realtime
                && std::env::var("OPENAI_API_KEY").is_err()
            {
                anyhow::bail!(
                    "OPENAI_API_KEY not set — required for realtime STT.\n\
                     Set it in .env or shell, or use --local for offline mode."
                );
            }

            // Auto-download Whisper model if whisper backend is selected
            #[cfg(feature = "whisper")]
            {
                if stt == supervox_agent::types::SttBackend::Whisper {
                    eprintln!("Checking Whisper model...");
                    crate::audio::ensure_whisper_model(&config)
                        .await
                        .map_err(|e| anyhow::anyhow!(e))?;
                }
            }

            // Pre-flight: check Parakeet model exists
            #[cfg(feature = "parakeet")]
            {
                if stt == supervox_agent::types::SttBackend::Parakeet {
                    // Check both default dir and life2film fallback
                    let default_dir = voxkit::parakeet_stt::default_model_dir();
                    let alt_dir =
                        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
                            .join("startups/active/life2film/video-analyzer/models/parakeet-tdt");
                    if !voxkit::parakeet_stt::model_exists(&default_dir)
                        && !voxkit::parakeet_stt::model_exists(&alt_dir)
                    {
                        anyhow::bail!(
                            "Parakeet model not found. Download:\n\
                             huggingface-cli download istupakov/parakeet-tdt-0.6b-v2-onnx \\\n  \
                             encoder-model.int8.onnx decoder_joint-model.int8.onnx nemo128.onnx vocab.txt config.json \\\n  \
                             --local-dir ~/.supervox/models/parakeet-tdt"
                        );
                    }
                }
            }
            app::run(app::Mode::Live).await?;
        }
        Some(Commands::Analyze { file, json }) => {
            if json {
                cmd_analyze_json(&file).await?;
            } else {
                app::run(app::Mode::Analysis { file }).await?;
            }
        }
        Some(Commands::Agent) => {
            app::run(app::Mode::Agent).await?;
        }
    }
    Ok(())
}

/// Check if Ollama is reachable; warn if not.
fn check_ollama_health() {
    use std::net::TcpStream;
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 11434));
    match TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(2)) {
        Ok(_) => {}
        Err(_) => {
            eprintln!("warning: Ollama not reachable at localhost:11434 — LLM calls may fail");
        }
    }
}

/// Play audio recording for a call via system player.
fn cmd_play(call_id: &str) -> Result<()> {
    let calls_dir = supervox_agent::storage::default_calls_dir();
    let call = supervox_agent::storage::load_call(&calls_dir, call_id)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if !supervox_agent::storage::has_audio(&calls_dir, &call) {
        anyhow::bail!("No audio recording for call {call_id}");
    }

    let wav_path = supervox_agent::storage::audio_path_for_call(&calls_dir, &call);
    eprintln!("Playing: {}", wav_path.display());

    std::process::Command::new("open")
        .arg(&wav_path)
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to open audio player: {e}"))?;

    Ok(())
}

/// Map file extension to MIME type for supported audio formats.
fn mime_for_extension(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "wav" => Some("audio/wav"),
        "mp3" => Some("audio/mpeg"),
        "m4a" => Some("audio/mp4"),
        "flac" => Some("audio/flac"),
        "ogg" => Some("audio/ogg"),
        "webm" => Some("audio/webm"),
        _ => None,
    }
}

/// Import an external audio file: transcribe → save as Call → optionally analyze.
async fn cmd_import(
    file: &str,
    no_analyze: bool,
    json: bool,
    language_override: Option<&str>,
) -> Result<()> {
    let path = std::path::Path::new(file);

    // Validate file exists
    if !path.exists() {
        anyhow::bail!("File not found: {file}");
    }

    // Validate extension and get MIME type
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let mime = mime_for_extension(ext).ok_or_else(|| {
        anyhow::anyhow!("Unsupported format: .{ext}\nSupported: wav, mp3, m4a, flac, ogg, webm")
    })?;

    let config_path = supervox_agent::storage::default_config_path();
    let config = supervox_agent::storage::load_config(&config_path)
        .map_err(|e| anyhow::anyhow!("Config error: {e}"))?;
    let language = language_override.unwrap_or(&config.my_language);

    let stt_backend = audio::effective_stt_backend(&config);

    eprintln!("Transcribing: {file} ({ext}, {mime})");

    // Transcribe based on backend
    let transcript = match stt_backend {
        supervox_agent::types::SttBackend::Whisper => {
            // Whisper only supports WAV
            if !ext.eq_ignore_ascii_case("wav") {
                anyhow::bail!(
                    "Whisper backend only supports WAV files. Convert first:\n  \
                     ffmpeg -i {file} output.wav"
                );
            }
            eprintln!("Using Whisper (local) backend...");

            #[cfg(feature = "whisper")]
            {
                let model_path = audio::ensure_whisper_model(&config)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))?;
                let chunk =
                    voxkit::read_wav_file(path).map_err(|e| anyhow::anyhow!("Read WAV: {e}"))?;
                voxkit::whisper_stt::WhisperStt::transcribe_file(&model_path, &chunk, language)
                    .map_err(|e| anyhow::anyhow!("Whisper transcribe: {e}"))?
            }
            #[cfg(not(feature = "whisper"))]
            {
                anyhow::bail!(
                    "Whisper feature not enabled. Use OpenAI backend or rebuild with --features whisper"
                );
            }
        }
        supervox_agent::types::SttBackend::Realtime => {
            // Use OpenAI batch API for import (not realtime streaming)
            eprintln!("Using OpenAI batch API...");
            let api_key = std::env::var("OPENAI_API_KEY")
                .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY not set"))?;
            let stt = voxkit::openai_stt::OpenAiStt::new(&api_key).with_language(language);
            let bytes = std::fs::read(path)?;
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("audio.wav");
            stt.transcribe_file_bytes(&bytes, filename, mime)
                .await
                .map_err(|e| anyhow::anyhow!("OpenAI transcribe: {e}"))?
        }
        supervox_agent::types::SttBackend::Parakeet => {
            eprintln!("Using Parakeet (local) backend...");

            #[cfg(feature = "parakeet")]
            {
                let chunk =
                    voxkit::read_wav_file(path).map_err(|e| anyhow::anyhow!("Read WAV: {e}"))?;
                let model_dir = voxkit::parakeet_stt::default_model_dir();
                let dir = if voxkit::parakeet_stt::model_exists(&model_dir) {
                    model_dir
                } else {
                    let alt = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
                        .join("startups/active/life2film/video-analyzer/models/parakeet-tdt");
                    if voxkit::parakeet_stt::model_exists(&alt) {
                        alt
                    } else {
                        anyhow::bail!("Parakeet model not found");
                    }
                };
                voxkit::parakeet_stt::ParakeetStt::transcribe_file(&dir, &chunk, language)
                    .map_err(|e| anyhow::anyhow!("Parakeet transcribe: {e}"))?
            }
            #[cfg(not(feature = "parakeet"))]
            {
                anyhow::bail!("Parakeet feature not enabled. Rebuild with --features parakeet");
            }
        }
    };

    eprintln!(
        "Transcribed: {} chars, {:.1}s duration",
        transcript.text.len(),
        transcript.duration_secs
    );

    // Create Call
    let call_id = uuid::Uuid::now_v7().to_string();
    let calls_dir = supervox_agent::storage::default_calls_dir();
    let now = chrono::Utc::now();
    let date = now.format("%Y%m%d");

    // Copy audio file to calls dir with canonical name
    std::fs::create_dir_all(&calls_dir)?;
    let dest_filename = format!("{date}-{call_id}.{ext}");
    let dest_path = calls_dir.join(&dest_filename);
    std::fs::copy(path, &dest_path)?;
    eprintln!("Audio copied to: {}", dest_path.display());

    let call = supervox_agent::types::Call {
        id: call_id.clone(),
        created_at: now,
        duration_secs: transcript.duration_secs,
        participants: Vec::new(),
        language: transcript.language.clone(),
        transcript: transcript.text.clone(),
        translation: None,
        tags: Vec::new(),
        audio_path: Some(dest_filename),
        bookmarks: Vec::new(),
    };

    supervox_agent::storage::save_call(&calls_dir, &call)
        .map_err(|e| anyhow::anyhow!("Save call: {e}"))?;
    eprintln!("Call saved: {call_id}");

    // Auto-analyze unless skipped
    if !no_analyze && !transcript.text.is_empty() {
        eprintln!("Analyzing...");
        match analysis_pipeline::analyze_transcript(&transcript.text, config.effective_model())
            .await
        {
            Ok(analysis) => {
                supervox_agent::storage::save_analysis(&calls_dir, &call_id, &analysis)
                    .map_err(|e| anyhow::anyhow!("Save analysis: {e}"))?;
                supervox_agent::storage::update_call_tags(&calls_dir, &call_id, &analysis.themes)
                    .ok();
                eprintln!(
                    "Analysis: {}",
                    analysis.summary.chars().take(80).collect::<String>()
                );
            }
            Err(e) => {
                eprintln!("Analysis failed (call still saved): {e}");
            }
        }
    }

    if json {
        // Re-load to get updated tags
        let final_call = supervox_agent::storage::load_call(&calls_dir, &call_id)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        println!("{}", serde_json::to_string_pretty(&final_call)?);
    } else {
        println!("Imported: {call_id}");
        println!("  Duration: {:.1}s", transcript.duration_secs);
        println!("  Transcript: {} chars", transcript.text.len());
        if let Some(lang) = &transcript.language {
            println!("  Language: {lang}");
        }
    }

    Ok(())
}

/// Delete a call by ID with confirmation prompt.
fn cmd_delete(call_id: &str, force: bool) -> Result<()> {
    let calls_dir = supervox_agent::storage::default_calls_dir();
    let call = supervox_agent::storage::load_call(&calls_dir, call_id)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if !force {
        let date = call.created_at.format("%Y-%m-%d %H:%M");
        let first_line = call
            .transcript
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(60)
            .collect::<String>();
        eprintln!("Delete call {call_id}?");
        eprintln!("  Date: {date}");
        eprintln!("  Preview: {first_line}");
        eprint!("Confirm (y/N): ");

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            eprintln!("Cancelled.");
            return Ok(());
        }
    }

    supervox_agent::storage::delete_call(&calls_dir, call_id)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    eprintln!("Deleted call {call_id}");
    Ok(())
}

/// Export a call as markdown to stdout or file.
fn cmd_export(call_id: &str, output: Option<&str>) -> Result<()> {
    let calls_dir = supervox_agent::storage::default_calls_dir();
    let call = supervox_agent::storage::load_call(&calls_dir, call_id)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let md = supervox_agent::storage::export_call_markdown(&call, None);

    match output {
        Some(path) => {
            std::fs::write(path, &md)?;
            eprintln!("Exported to {path}");
        }
        None => {
            print!("{md}");
        }
    }
    Ok(())
}

/// Search call transcripts and display matches.
fn cmd_search(query: &str, json: bool, filter_args: &FilterArgs) -> Result<()> {
    let calls_dir = supervox_agent::storage::default_calls_dir();
    let call_filter = filter_args.to_call_filter()?;
    let has_filter =
        !call_filter.tags.is_empty() || call_filter.since.is_some() || call_filter.until.is_some();

    let mut matches = supervox_agent::tools::search::search_calls_in_dir(&calls_dir, query)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Post-filter search results if filter flags are set
    if has_filter {
        let calls =
            supervox_agent::storage::list_calls(&calls_dir).map_err(|e| anyhow::anyhow!("{e}"))?;
        let filtered = supervox_agent::storage::filter_calls(&calls, &call_filter);
        let filtered_ids: std::collections::HashSet<String> =
            filtered.into_iter().map(|c| c.id).collect();
        matches.retain(|m| filtered_ids.contains(&m.call_id));
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&matches)?);
        return Ok(());
    }

    if matches.is_empty() {
        println!("No matches for \"{query}\"");
        return Ok(());
    }

    for m in &matches {
        println!("--- {} (score: {:.4}) ---", m.call_id, m.score);
        println!("{}", m.snippet);
        println!();
    }
    println!("{} match(es) found", matches.len());
    Ok(())
}

/// List saved calls to stdout (non-TUI).
fn cmd_calls(json: bool, filter_args: &FilterArgs) -> Result<()> {
    let calls_dir = supervox_agent::storage::default_calls_dir();
    let all_calls =
        supervox_agent::storage::list_calls(&calls_dir).map_err(|e| anyhow::anyhow!("{e}"))?;
    let call_filter = filter_args.to_call_filter()?;
    let calls = supervox_agent::storage::filter_calls(&all_calls, &call_filter);

    if json {
        println!("{}", serde_json::to_string_pretty(&calls)?);
        return Ok(());
    }

    if calls.is_empty() {
        println!("No calls found");
        return Ok(());
    }

    println!(
        "{:<20} {:>8} {:<3} {:<3} FIRST LINE",
        "DATE", "DURATION", "AN", "\u{266b}"
    );
    println!("{}", "-".repeat(70));

    for call in &calls {
        let date = call.created_at.format("%Y-%m-%d %H:%M");
        let duration = format!("{}s", call.duration_secs as u64);
        let has_analysis = supervox_agent::storage::load_analysis(&calls_dir, &call.id)
            .ok()
            .flatten()
            .is_some();
        let analysis_indicator = if has_analysis { "\u{2713}" } else { "\u{2717}" };
        let audio_indicator = if supervox_agent::storage::has_audio(&calls_dir, call) {
            "\u{266b}"
        } else {
            " "
        };
        let first_line = call
            .transcript
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(40)
            .collect::<String>();
        println!(
            "{:<20} {:>8}  {}   {}  {}",
            date, duration, analysis_indicator, audio_indicator, first_line
        );
    }

    let total = all_calls.len();
    let shown = calls.len();
    if shown < total {
        println!("\n{shown}/{total} call(s) (filtered)");
    } else {
        println!("\n{total} call(s) total");
    }
    Ok(())
}

/// Run analysis on a call file and output JSON (non-TUI).
async fn cmd_analyze_json(file: &str) -> Result<()> {
    let json_str = std::fs::read_to_string(file)?;
    let call: supervox_agent::types::Call = serde_json::from_str(&json_str)?;

    if call.transcript.is_empty() {
        anyhow::bail!("Call has no transcript to analyze");
    }

    let config_path = supervox_agent::storage::default_config_path();
    let config = supervox_agent::storage::load_config(&config_path)
        .map_err(|e| anyhow::anyhow!("Config error: {e}"))?;

    let analysis =
        analysis_pipeline::analyze_transcript(&call.transcript, config.effective_model())
            .await
            .map_err(|e| anyhow::anyhow!("Analysis failed: {e}"))?;
    println!("{}", serde_json::to_string_pretty(&analysis)?);
    Ok(())
}

/// Display aggregate call statistics.
fn cmd_stats(json: bool, filter_args: &FilterArgs) -> Result<()> {
    let calls_dir = supervox_agent::storage::default_calls_dir();
    let call_filter = filter_args.to_call_filter()?;
    let has_filter =
        !call_filter.tags.is_empty() || call_filter.since.is_some() || call_filter.until.is_some();

    let stats = if has_filter {
        let all_calls =
            supervox_agent::storage::list_calls(&calls_dir).map_err(|e| anyhow::anyhow!("{e}"))?;
        let filtered = supervox_agent::storage::filter_calls(&all_calls, &call_filter);
        compute_stats_from_calls(&calls_dir, &filtered)
    } else {
        supervox_agent::storage::compute_stats(&calls_dir).map_err(|e| anyhow::anyhow!("{e}"))?
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
        return Ok(());
    }

    let total_secs = stats.total_duration_secs as u64;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;

    println!(
        "Call Statistics{}",
        if has_filter { " (filtered)" } else { "" }
    );
    println!("{}", "=".repeat(40));
    println!("Total calls:      {}", stats.total_calls);
    println!("Total duration:   {hours}h {mins}m");
    println!(
        "Analysis coverage: {}/{} ({:.0}%)",
        stats.analyzed_count,
        stats.total_calls,
        if stats.total_calls > 0 {
            stats.analyzed_count as f64 / stats.total_calls as f64 * 100.0
        } else {
            0.0
        }
    );
    println!("This week:        {}", stats.calls_this_week);
    println!("This month:       {}", stats.calls_this_month);

    if !stats.top_themes.is_empty() {
        println!("\nTop Themes:");
        for t in &stats.top_themes {
            println!("  {} ({}x)", t.theme, t.count);
        }
    }

    Ok(())
}

/// Compute stats from a pre-filtered list of calls.
fn compute_stats_from_calls(
    calls_dir: &std::path::Path,
    calls: &[supervox_agent::types::Call],
) -> supervox_agent::types::CallStats {
    let now = chrono::Utc::now();
    let week_ago = now - chrono::Duration::days(7);
    let month_ago = now - chrono::Duration::days(30);

    let total_calls = calls.len();
    let total_duration_secs: f64 = calls.iter().map(|c| c.duration_secs).sum::<f64>().max(0.0);
    let calls_this_week = calls.iter().filter(|c| c.created_at >= week_ago).count();
    let calls_this_month = calls.iter().filter(|c| c.created_at >= month_ago).count();

    let mut analyzed_count = 0usize;
    let mut theme_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for call in calls {
        if let Ok(Some(analysis)) = supervox_agent::storage::load_analysis(calls_dir, &call.id) {
            analyzed_count += 1;
            for theme in &analysis.themes {
                *theme_counts.entry(theme.clone()).or_default() += 1;
            }
        }
    }

    let mut top_themes: Vec<supervox_agent::types::ThemeCount> = theme_counts
        .into_iter()
        .map(|(theme, count)| supervox_agent::types::ThemeCount { theme, count })
        .collect();
    top_themes.sort_by(|a, b| b.count.cmp(&a.count));
    top_themes.truncate(5);

    supervox_agent::types::CallStats {
        total_calls,
        total_duration_secs,
        analyzed_count,
        unanalyzed_count: total_calls - analyzed_count,
        top_themes,
        calls_this_week,
        calls_this_month,
    }
}

/// List all unique tags with counts.
fn cmd_tags(json: bool) -> Result<()> {
    let calls_dir = supervox_agent::storage::default_calls_dir();
    let calls =
        supervox_agent::storage::list_calls(&calls_dir).map_err(|e| anyhow::anyhow!("{e}"))?;
    let tags = supervox_agent::storage::collect_tags(&calls);

    if json {
        println!("{}", serde_json::to_string_pretty(&tags)?);
        return Ok(());
    }

    if tags.is_empty() {
        println!("No tags found. Analyze calls first to auto-tag from themes.");
        return Ok(());
    }

    println!("{:<30} COUNT", "TAG");
    println!("{}", "-".repeat(40));
    for t in &tags {
        println!("{:<30} {}", t.theme, t.count);
    }
    println!("\n{} unique tag(s)", tags.len());
    Ok(())
}

/// Batch-analyze all calls missing analysis files.
async fn cmd_analyze_all(dry_run: bool) -> Result<()> {
    let calls_dir = supervox_agent::storage::default_calls_dir();
    let calls =
        supervox_agent::storage::list_calls(&calls_dir).map_err(|e| anyhow::anyhow!("{e}"))?;

    let unanalyzed: Vec<_> = calls
        .into_iter()
        .filter(|c| {
            supervox_agent::storage::load_analysis(&calls_dir, &c.id)
                .ok()
                .flatten()
                .is_none()
        })
        .collect();

    if unanalyzed.is_empty() {
        println!("All calls have analysis cached.");
        return Ok(());
    }

    if dry_run {
        println!("{} call(s) without analysis:", unanalyzed.len());
        for call in &unanalyzed {
            let date = call.created_at.format("%Y-%m-%d %H:%M");
            let preview: String = call.transcript.chars().take(60).collect();
            println!("  {} {} — {}", call.id, date, preview);
        }
        return Ok(());
    }

    let config_path = supervox_agent::storage::default_config_path();
    let config = supervox_agent::storage::load_config(&config_path)
        .map_err(|e| anyhow::anyhow!("Config error: {e}"))?;
    let model = config.effective_model().to_string();
    let total = unanalyzed.len();

    for (i, call) in unanalyzed.iter().enumerate() {
        eprintln!("[{}/{}] Analyzing {}...", i + 1, total, call.id);

        if call.transcript.is_empty() {
            eprintln!("  Skipped (empty transcript)");
            continue;
        }

        match analysis_pipeline::analyze_transcript(&call.transcript, &model).await {
            Ok(analysis) => {
                if let Err(e) =
                    supervox_agent::storage::save_analysis(&calls_dir, &call.id, &analysis)
                {
                    eprintln!("  Failed to save analysis: {e}");
                    continue;
                }
                supervox_agent::storage::update_call_tags(&calls_dir, &call.id, &analysis.themes)
                    .ok();
                eprintln!(
                    "  Done: {}",
                    analysis.summary.chars().take(80).collect::<String>()
                );
            }
            Err(e) => {
                eprintln!("  Analysis failed: {e}");
            }
        }
    }

    eprintln!("Batch analysis complete.");
    Ok(())
}

/// List tracked action items across calls.
fn cmd_actions(include_completed: bool, json: bool, filter_args: &FilterArgs) -> Result<()> {
    let calls_dir = supervox_agent::storage::default_calls_dir();
    let actions_path = supervox_agent::storage::default_actions_path();
    let call_filter = filter_args.to_call_filter()?;

    let actions = supervox_agent::storage::list_tracked_actions(
        &calls_dir,
        &actions_path,
        &call_filter,
        include_completed,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    if json {
        println!("{}", serde_json::to_string_pretty(&actions)?);
        return Ok(());
    }

    if actions.is_empty() {
        if include_completed {
            println!("No action items found. Analyze calls first.");
        } else {
            println!("No open action items. Use --all to include completed.");
        }
        return Ok(());
    }

    // Group by call date
    let mut current_call = String::new();
    for action in &actions {
        if action.call_id != current_call {
            current_call = action.call_id.clone();
            let date = action.call_date.format("%Y-%m-%d %H:%M");
            println!("\n{date} ({}):", action.call_id);
        }
        let check = if action.state.completed {
            "\u{2611}" // ☑
        } else {
            "\u{2610}" // ☐
        };
        let mut line = format!("  {check} [{}] {}", action.action_id, action.description);
        if let Some(who) = &action.assignee {
            line.push_str(&format!(" (@{who})"));
        }
        if let Some(when) = &action.deadline {
            line.push_str(&format!(" — due {when}"));
        }
        println!("{line}");
    }

    let total = actions.len();
    let completed = actions.iter().filter(|a| a.state.completed).count();
    let open = total - completed;
    println!("\n{open} open, {completed} completed, {total} total");
    Ok(())
}

/// Mark an action as done by ID prefix.
fn cmd_action_done(prefix: &str) -> Result<()> {
    let calls_dir = supervox_agent::storage::default_calls_dir();
    let actions_path = supervox_agent::storage::default_actions_path();

    let action = supervox_agent::storage::find_action_by_prefix(&calls_dir, &actions_path, prefix)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    match action {
        Some(a) => {
            supervox_agent::storage::set_action_completed(&actions_path, &a.action_id)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("\u{2611} Done: {} [{}]", a.description, a.action_id);
        }
        None => {
            anyhow::bail!("No action found matching prefix \"{prefix}\"");
        }
    }
    Ok(())
}

/// Undo action completion by ID prefix.
fn cmd_action_undo(prefix: &str) -> Result<()> {
    let calls_dir = supervox_agent::storage::default_calls_dir();
    let actions_path = supervox_agent::storage::default_actions_path();

    let action = supervox_agent::storage::find_action_by_prefix(&calls_dir, &actions_path, prefix)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    match action {
        Some(a) => {
            supervox_agent::storage::set_action_incomplete(&actions_path, &a.action_id)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("\u{2610} Undone: {} [{}]", a.description, a.action_id);
        }
        None => {
            anyhow::bail!("No action found matching prefix \"{prefix}\"");
        }
    }
    Ok(())
}

/// Generate cross-call insights and display formatted output or JSON.
async fn cmd_insights(json: bool, filter_args: &FilterArgs) -> Result<()> {
    let config_path = supervox_agent::storage::default_config_path();
    let config = supervox_agent::storage::load_config(&config_path)
        .map_err(|e| anyhow::anyhow!("Config error: {e}"))?;

    let call_filter = filter_args.to_call_filter()?;

    eprintln!("Generating insights from call history...");
    let insights =
        analysis_pipeline::generate_insights_filtered(config.effective_model(), &call_filter)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

    if json {
        println!("{}", serde_json::to_string_pretty(&insights)?);
        return Ok(());
    }

    println!(
        "Cross-call Insights ({} calls, {})",
        insights.total_calls, insights.period
    );
    println!("{}", "=".repeat(50));

    if !insights.recurring_themes.is_empty() {
        println!("\nRecurring Themes:");
        for t in &insights.recurring_themes {
            println!("  {} ({}x)", t.theme, t.count);
        }
    }

    let ms = &insights.mood_summary;
    println!(
        "\nMood Summary: +{} neutral:{} -{} mixed:{}",
        ms.positive, ms.neutral, ms.negative, ms.mixed
    );

    if !insights.open_action_items.is_empty() {
        println!("\nOpen Action Items:");
        for a in &insights.open_action_items {
            let mut line = format!("  - {}", a.description);
            if let Some(who) = &a.assignee {
                line.push_str(&format!(" (@{who})"));
            }
            if let Some(when) = &a.deadline {
                line.push_str(&format!(" — due {when}"));
            }
            println!("{line}");
        }
    }

    if !insights.key_patterns.is_empty() {
        println!("\nKey Patterns:");
        for p in &insights.key_patterns {
            println!("  • {p}");
        }
    }

    Ok(())
}
