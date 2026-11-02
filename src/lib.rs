//! Shared application core and verified transactional cache.
pub mod discovery;
mod files;
mod fingerprint;
pub mod mcp_inventory;
mod plugins;
pub mod presentation;
mod provenance;
pub mod tui;
mod worktree;

use rusqlite::{Connection, OpenFlags, params};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SCHEMA_VERSION: u32 = 1;
const INDEX_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub struct AppError {
    pub code: &'static str,
    pub message: &'static str,
}

impl AppError {
    pub const fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

#[derive(Debug, Clone, Serialize)]
pub struct Paths {
    pub home: PathBuf,
    pub project: PathBuf,
    pub state_dir: PathBuf,
}

impl Paths {
    /// Explicit home selection also isolates the default state location from XDG.
    pub fn resolve(
        home: Option<PathBuf>,
        project: Option<PathBuf>,
        state_dir: Option<PathBuf>,
    ) -> Result<Self, AppError> {
        let explicit_home = home.is_some();
        let home = home
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .ok_or(AppError::new(
                "invalid_home",
                "Supply --home; no home directory is available.",
            ))?;
        let home = existing_directory(&home, "invalid_home")?;
        let project = match project {
            Some(path) => path,
            None => std::env::current_dir().map_err(|_| {
                AppError::new(
                    "invalid_project",
                    "Supply --project; current directory is unavailable.",
                )
            })?,
        };
        let project = existing_directory(&project, "invalid_project")?;
        let state_dir = state_dir.unwrap_or_else(|| default_state_dir(&home, explicit_home));
        let state_dir = absolute(&state_dir)?;
        if state_dir.to_str().is_none() {
            return Err(AppError::new(
                "invalid_state_dir",
                "State directory must be a UTF-8 path.",
            ));
        }
        // Resolve/validate all read roots before creating application state.
        std::fs::create_dir_all(&state_dir).map_err(|_| {
            AppError::new(
                "invalid_state_dir",
                "Cannot create or access the state directory.",
            )
        })?;
        let state_dir = existing_directory(&state_dir, "invalid_state_dir")?;
        Ok(Self {
            home,
            project,
            state_dir,
        })
    }
}

fn absolute(path: &Path) -> Result<PathBuf, AppError> {
    std::path::absolute(path)
        .map_err(|_| AppError::new("invalid_path", "Cannot resolve the selected path."))
}

fn existing_directory(path: &Path, code: &'static str) -> Result<PathBuf, AppError> {
    let path = path.canonicalize().map_err(|_| {
        AppError::new(
            code,
            "Selected directory does not exist or is inaccessible.",
        )
    })?;
    if !path.is_dir() || path.to_str().is_none() {
        return Err(AppError::new(
            code,
            "Selected path must be a UTF-8 directory.",
        ));
    }
    Ok(path)
}

fn default_state_dir(home: &Path, explicit_home: bool) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let _ = explicit_home;
        home.join("Library/Application Support/skillrail")
    }
    #[cfg(not(target_os = "macos"))]
    {
        if !explicit_home
            && let Some(path) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from)
            && path.is_absolute()
        {
            return path.join("skillrail");
        }
        home.join(".local/share/skillrail")
    }
}

#[derive(Debug, Serialize)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<DiagnosticLocation>,
}

#[derive(Debug, Serialize)]
pub struct DiagnosticLocation {
    pub path: PathBuf,
    pub agent: discovery::Agent,
    pub scope: discovery::Scope,
    pub category: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Discovery {
    pub status: String,
    pub adapters: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Inventory {
    pub schema_version: u32,
    pub context: Paths,
    pub scanned_at_unix_ms: u64,
    pub discovery: Discovery,
    pub items: Vec<discovery::Item>,
    pub occurrences: Vec<discovery::Occurrence>,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct Application {
    paths: Paths,
}

impl Application {
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }

    /// Both list and scan rebuild from currently implemented sources, never stale rows.
    pub fn scan(&self) -> Result<Inventory, AppError> {
        let scanned_at_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| {
                AppError::new("clock_unavailable", "System time precedes the Unix epoch.")
            })?
            .as_millis()
            .try_into()
            .map_err(|_| AppError::new("clock_unavailable", "System time is out of range."))?;
        let inventory = discovery::discover(&self.paths, scanned_at_unix_ms);
        self.replace_index(&inventory)?;
        Ok(inventory)
    }

    fn replace_index(&self, inventory: &Inventory) -> Result<(), AppError> {
        let unavailable = || {
            AppError::new(
                "index_unavailable",
                "Cannot rebuild index.sqlite3; check state access or remove the cache and retry.",
            )
        };
        // SQLite must not follow a substituted cache symlink.
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NOFOLLOW;
        let mut connection =
            Connection::open_with_flags(self.paths.state_dir.join("index.sqlite3"), flags)
                .map_err(|_| unavailable())?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| unavailable())?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|_| unavailable())?;
        let version: i64 = transaction
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|_| unavailable())?;
        if version != 0 && version != i64::from(INDEX_VERSION) {
            return Err(AppError::new(
                "index_version_unsupported",
                "Unsupported cache schema; remove index.sqlite3 and retry.",
            ));
        }
        transaction
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS scan (
                    id INTEGER PRIMARY KEY CHECK(id = 1),
                    inventory_json TEXT NOT NULL
                );
                DELETE FROM scan;",
            )
            .map_err(|_| unavailable())?;
        let json = serde_json::to_string(inventory)
            .map_err(|_| AppError::new("output_unavailable", "Cannot encode inventory."))?;
        let inserted = transaction
            .execute(
                "INSERT INTO scan (id, inventory_json) VALUES (1, ?1)",
                params![json],
            )
            .map_err(|_| unavailable())?;
        // An altered schema can ignore or rewrite INSERT without returning an error.
        let (rows, stored): (i64, String) = transaction
            .query_row(
                "SELECT (SELECT count(*) FROM scan), inventory_json FROM scan WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| unavailable())?;
        if inserted != 1 || rows != 1 || stored != json {
            return Err(unavailable());
        }
        transaction
            .pragma_update(None, "user_version", INDEX_VERSION)
            .map_err(|_| unavailable())?;
        transaction.commit().map_err(|_| unavailable())
    }
}
