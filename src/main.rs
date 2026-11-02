use clap::{Parser, Subcommand};
use std::io::Write;
use std::path::PathBuf;

mod output;

#[derive(Parser)]
#[command(
    version,
    about = "Local-first skill and MCP registry (standalone skill inventory)"
)]
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
    /// Interactive skill inventory (requires a terminal).
    Tui,
    /// Fresh standalone skill inventory and explanations.
    Scan {
        #[arg(long)]
        json: bool,
    },
    /// Fresh standalone skill inventory and explanations.
    List {
        #[arg(long)]
        json: bool,
    },
}

fn main() -> std::process::ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) if !error.use_stderr() => {
            return output::finish(output::stdout(&error.to_string()), false);
        }
        Err(error) => {
            let _ = write!(std::io::stderr().lock(), "{error}");
            return std::process::ExitCode::from(error.exit_code() as u8);
        }
    };
    if matches!(cli.command, Some(Commands::Version)) {
        return output::finish(
            output::stdout(&format!("skillrail {}\n", env!("CARGO_PKG_VERSION"))),
            false,
        );
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
                output::stdout(&format!("{output}\n"))?;
            } else {
                output::stdout(&format!(
                    "Standalone skill inventory: {} items (partial coverage).\nHome: {}\nProject: {}\nState: {}\nLast scan: {} (Unix ms)\n",
                    inventory.items.len(),
                    output::human_path(&inventory.context.home),
                    output::human_path(&inventory.context.project),
                    output::human_path(&inventory.context.state_dir),
                    inventory.scanned_at_unix_ms,
                ))?;
                for index in 0..inventory.items.len() {
                    output::stdout(&skillrail::presentation::details(&inventory, index))?;
                }
                for diagnostic in &inventory.diagnostics {
                    output::stdout(&format!("{}: {}\n", diagnostic.code, diagnostic.message))?;
                }
            }
        }
        Ok(())
    })();
    output::finish(result, json)
}
