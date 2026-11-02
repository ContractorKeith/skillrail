use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "Local-first skill and MCP registry (M0 harness)")]
struct Cli {
    /// Selected home directory (defaults to HOME).
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    /// Selected project directory (defaults to the current directory).
    #[arg(long, global = true)]
    project: Option<PathBuf>,
    /// Application state directory (see docs/CLI.md for platform defaults).
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Print the application version without opening state.
    Version,
    /// Interactive empty inventory (requires a terminal).
    Tui,
    /// Fresh inventory; discovery adapters arrive after M0.
    Scan {
        #[arg(long)]
        json: bool,
    },
    /// Fresh inventory; discovery adapters arrive after M0.
    List {
        #[arg(long)]
        json: bool,
    },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    if matches!(cli.command, Some(Commands::Version)) {
        println!("skillrail {}", env!("CARGO_PKG_VERSION"));
        return std::process::ExitCode::SUCCESS;
    }
    let json = matches!(
        cli.command,
        Some(Commands::Scan { json: true } | Commands::List { json: true })
    );
    let interactive = matches!(cli.command, None | Some(Commands::Tui));
    let result: Result<(), skillrail::AppError> = (|| {
        if interactive {
            skillrail::tui::require_terminal()?;
        }
        let paths = skillrail::Paths::resolve(cli.home, cli.project, cli.state_dir)?;
        let application = skillrail::Application::new(paths);
        if interactive {
            skillrail::tui::run(&application)?;
        } else {
            let inventory = application.scan()?;
            if json {
                let output = serde_json::to_string(&inventory).map_err(|_| {
                    skillrail::AppError::new("output_unavailable", "Cannot encode inventory.")
                })?;
                println!("{output}");
            } else {
                println!("M0: discovery not implemented; 0 indexed items.");
                println!("Home: {}", inventory.context.home.display());
                println!("Project: {}", inventory.context.project.display());
                println!("State: {}", inventory.context.state_dir.display());
                println!("Last scan: {} (Unix ms)", inventory.scanned_at_unix_ms);
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            if json {
                eprintln!(
                    "{}",
                    serde_json::json!({"schema_version": skillrail::SCHEMA_VERSION, "error": error})
                );
            } else {
                eprintln!("{error}");
            }
            std::process::ExitCode::from(1)
        }
    }
}
