use clap::{Parser, Subcommand};
use std::io::Write;
use std::path::PathBuf;

mod output;

#[derive(Parser)]
#[command(
    version,
    about = "Local-first skill and MCP registry with reversible visibility changes"
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
    /// Interactive skill and MCP inventory (requires a terminal).
    Tui,
    /// Fresh skill, MCP and plugin inventory and explanations.
    Scan {
        #[arg(long)]
        json: bool,
    },
    /// Fresh skill, MCP and plugin inventory and explanations.
    List {
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        query: skillrail::query::Query,
    },
    /// Search names, descriptions and IDs (case insensitive).
    Search {
        query: String,
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        filters: skillrail::query::Query,
    },
    /// Inspect one stable item ID.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// List drifted skills and skills with unknown drift.
    Drift {
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        query: skillrail::query::Query,
    },
    /// Preview enabling a supported occurrence; writes only with --apply.
    Enable(ToggleArgs),
    /// Preview disabling a supported occurrence; writes only with --apply.
    Disable(ToggleArgs),
    /// List sanitized durable operation history (independent of the cache).
    Operations {
        #[arg(long)]
        json: bool,
    },
    /// Preview conflict-aware restoration; writes only with --apply.
    Undo {
        operation_id: String,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        json: bool,
    },
    /// Serve read-only MCP over stdio (protocol output only).
    Mcp,
}

#[derive(clap::Args)]
struct ToggleArgs {
    /// Stable item or exact occurrence ID from scan/show.
    id: String,
    #[arg(long, value_parser = ["claude", "codex"])]
    agent: String,
    /// Scope of the selected occurrence (required; never inferred).
    #[arg(long, value_parser = ["user", "project", "local"])]
    scope: String,
    #[arg(long)]
    apply: bool,
    #[arg(long)]
    json: bool,
    /// Refuse if the newly computed preview differs from this earlier token.
    #[arg(long)]
    expect: Option<String>,
}
fn mutation_output(value: &impl serde::Serialize, json: bool) -> Result<(), skillrail::AppError> {
    let encoded = if json {
        serde_json::to_string(value)
    } else {
        serde_json::to_string_pretty(value)
    }
    .map_err(|_| {
        skillrail::AppError::new("output_unavailable", "Cannot encode operation output.")
    })?;
    output::stdout(&format!(
        "{}\n",
        if json {
            encoded
        } else {
            encoded
                .split('\n')
                .map(skillrail::presentation::safe)
                .collect::<Vec<_>>()
                .join("\n")
        }
    ))
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
        Some(
            Commands::Scan { json: true }
                | Commands::List { json: true, .. }
                | Commands::Search { json: true, .. }
                | Commands::Show { json: true, .. }
                | Commands::Drift { json: true, .. }
                | Commands::Enable(ToggleArgs { json: true, .. })
                | Commands::Disable(ToggleArgs { json: true, .. })
                | Commands::Operations { json: true }
                | Commands::Undo { json: true, .. }
        )
    );
    let interactive = matches!(cli.command, None | Some(Commands::Tui));
    let result: Result<(), skillrail::AppError> = (|| {
        if interactive {
            skillrail::tui::require_terminal()?;
        }
        let query = match &cli.command {
            Some(Commands::List { query, .. }) => Some(query.clone()),
            Some(Commands::Search { query, filters, .. }) => Some(skillrail::query::Query {
                query: Some(query.clone()),
                ..filters.clone()
            }),
            Some(Commands::Show { id, .. }) => Some(skillrail::query::Query {
                id: Some(id.clone()),
                ..Default::default()
            }),
            Some(Commands::Drift { query, .. }) => Some(skillrail::query::Query {
                drift_only: true,
                ..query.clone()
            }),
            _ => None,
        };
        if let Some(query) = &query {
            query.validate()?;
        }
        let paths = skillrail::Paths::resolve(cli.home, cli.project, cli.state_dir)?;
        let application = skillrail::Application::new(paths);
        if let Some(Commands::Enable(args) | Commands::Disable(args)) = &cli.command {
            let plan = application.plan_toggle(skillrail::mutation::ToggleRequest {
                id: args.id.clone(),
                agent: args.agent.clone(),
                scope: args.scope.clone(),
                enabled: matches!(cli.command, Some(Commands::Enable(_))),
            })?;
            if args
                .expect
                .as_ref()
                .is_some_and(|token| token != &plan.view().token)
            {
                return Err(skillrail::AppError::new(
                    "mutation_conflict",
                    "Preview changed; inspect a fresh preview before applying.",
                ));
            }
            let result = if args.apply {
                application.apply(&plan)?
            } else {
                plan.view().clone()
            };
            mutation_output(&result, json)?;
        } else if let Some(Commands::Operations { .. }) = &cli.command {
            mutation_output(&application.operations()?, json)?;
        } else if let Some(Commands::Undo {
            operation_id,
            apply,
            ..
        }) = &cli.command
        {
            let plan = application.plan_undo(operation_id)?;
            let result = if *apply {
                application.apply(&plan)?
            } else {
                plan.view().clone()
            };
            mutation_output(&result, json)?;
        } else if interactive {
            skillrail::tui::run(&application)?;
        } else if matches!(cli.command, Some(Commands::Mcp)) {
            skillrail::mcp_server::run(application)?;
        } else {
            let (inventory, encoded, page) = if let Some(query) = query {
                let result = application.query(&query)?;
                let encoded = skillrail::query::encode(&result)?;
                (result.inventory, encoded, Some(result.page))
            } else {
                let inventory = application.scan()?;
                let encoded = serde_json::to_string(&inventory).map_err(|_| {
                    skillrail::AppError::new("output_unavailable", "Cannot encode inventory.")
                })?;
                (inventory, encoded, None)
            };
            if json {
                let output = encoded;
                output::stdout(&format!("{output}\n"))?;
            } else {
                output::stdout(&format!(
                    "Skill/MCP inventory: {} items (partial coverage).\nHome: {}\nProject: {}\nState: {}\nLast scan: {} (Unix ms)\n",
                    inventory.items.len(),
                    output::human_path(&inventory.context.home),
                    output::human_path(&inventory.context.project),
                    output::human_path(&inventory.context.state_dir),
                    inventory.scanned_at_unix_ms,
                ))?;
                for index in 0..inventory.items.len() {
                    output::stdout(&skillrail::presentation::details(&inventory, index))?;
                }
                if let Some(page) = page
                    && (page.next_offset.is_some() || page.offset > 0)
                {
                    output::stdout(&format!(
                        "Page: offset {} of {} | revision {} | next offset {}\n",
                        page.offset,
                        page.total,
                        page.revision,
                        page.next_offset
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "none".into())
                    ))?;
                }
                for diagnostic in &inventory.diagnostics {
                    output::stdout(&format!(
                        "{}\n",
                        skillrail::presentation::diagnostic(diagnostic)
                    ))?;
                }
            }
        }
        Ok(())
    })();
    output::finish(result, json)
}
