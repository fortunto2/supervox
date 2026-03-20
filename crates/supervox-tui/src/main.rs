use anyhow::Result;
use clap::{Parser, Subcommand};

mod agent_loop;
mod analysis_pipeline;
mod app;
mod audio;
mod clipboard;
mod intelligence;
mod modes;

#[derive(Parser)]
#[command(name = "supervox", about = "Voice-powered productivity TUI")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Live call assistant — real-time subtitles + translation + rolling summary
    Live,
    /// Post-call analysis — summary, action items, follow-up draft
    Analyze {
        /// Path to call JSON file
        file: String,
    },
    /// Agent chat — Q&A over call history
    Agent,
    /// List past calls
    Calls,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Calls) => {
            cmd_calls()?;
        }
        Some(Commands::Live) | None => {
            app::run(app::Mode::Live).await?;
        }
        Some(Commands::Analyze { file }) => {
            app::run(app::Mode::Analysis { file }).await?;
        }
        Some(Commands::Agent) => {
            app::run(app::Mode::Agent).await?;
        }
    }
    Ok(())
}

/// List saved calls to stdout (non-TUI).
fn cmd_calls() -> Result<()> {
    let calls_dir = supervox_agent::storage::default_calls_dir();
    let calls =
        supervox_agent::storage::list_calls(&calls_dir).map_err(|e| anyhow::anyhow!("{e}"))?;

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
