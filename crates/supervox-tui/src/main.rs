use anyhow::Result;
use clap::{Parser, Subcommand};

mod agent_loop;
mod analysis_pipeline;
mod app;
mod audio;
mod clipboard;
mod help;
mod intelligence;
mod modes;

#[derive(Parser)]
#[command(name = "supervox", about = "Voice-powered productivity TUI")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Use local Ollama LLM instead of cloud model
    #[arg(long, global = true)]
    local: bool,
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
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Apply --local override: switch to Ollama backend
    if cli.local {
        // SAFETY: set_var before any threads are spawned (single-threaded at this point)
        unsafe { std::env::set_var("SUPERVOX_LLM_BACKEND", "ollama") };
        check_ollama_health();
    }

    match cli.command {
        Some(Commands::Calls { json }) => {
            cmd_calls(json)?;
        }
        Some(Commands::Live) | None => {
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
    match TcpStream::connect_timeout(
        &"127.0.0.1:11434".parse().unwrap(),
        std::time::Duration::from_secs(2),
    ) {
        Ok(_) => {}
        Err(_) => {
            eprintln!("warning: Ollama not reachable at localhost:11434 — LLM calls may fail");
        }
    }
}

/// List saved calls to stdout (non-TUI).
fn cmd_calls(json: bool) -> Result<()> {
    let calls_dir = supervox_agent::storage::default_calls_dir();
    let calls =
        supervox_agent::storage::list_calls(&calls_dir).map_err(|e| anyhow::anyhow!("{e}"))?;

    if json {
        println!("{}", serde_json::to_string_pretty(&calls)?);
        return Ok(());
    }

    if calls.is_empty() {
        println!("No calls found in {}", calls_dir.display());
        return Ok(());
    }

    println!("{:<20} {:>8} FIRST LINE", "DATE", "DURATION");
    println!("{}", "-".repeat(60));

    for call in &calls {
        let date = call.created_at.format("%Y-%m-%d %H:%M");
        let duration = format!("{}s", call.duration_secs as u64);
        let first_line = call
            .transcript
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(40)
            .collect::<String>();
        println!("{:<20} {:>8} {}", date, duration, first_line);
    }

    println!("\n{} call(s) total", calls.len());
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
