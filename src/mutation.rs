//! Shared guarded visibility mutations. Public views cannot serialize recovery content.
mod journal_record;
use crate::{
    AppError, Application, Paths, config_patch,
    discovery::{Agent, Scope},
    guarded_fs::{Directory, Identity, Snapshot, filename},
    mcp_inventory::{ConfiguredStatus, Origin},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

pub(crate) fn refused(message: &'static str) -> AppError {
    AppError::new("mutation_refused", message)
}
fn conflict() -> AppError {
    AppError::new(
        "mutation_conflict",
        "Files changed since preview or publication; intervening edits were preserved. Refresh the preview or inspect operations for recovery.",
    )
}
fn journal_error() -> AppError {
    AppError::new(
        "journal_invalid",
        "Private journal is malformed, unsafe or has an unsupported version; preserve it for recovery. Do not rebuild it as a cache.",
    )
}
fn random_id() -> Result<String, AppError> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|_| journal_error())?;
    Ok(format!(
        "op-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}
fn valid_id(id: &str) -> bool {
    id.len() == 35
        && id.starts_with("op-")
        && id[3..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToggleRequest {
    pub id: String,
    pub agent: String,
    pub scope: String,
    pub enabled: bool,
}
impl ToggleRequest {
    pub fn validate(&self) -> Result<(), AppError> {
        if !["claude", "codex"].contains(&self.agent.as_str())
            || !["user", "project", "local"].contains(&self.scope.as_str())
            || self.id.is_empty()
            || self.id.len() > 256
        {
            return Err(AppError::new(
                "invalid_arguments",
                "Supply a stable item/occurrence ID, --agent claude|codex and --scope user|project|local.",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    path: PathBuf,
    parents: Vec<Identity>,
    before: Option<Snapshot>,
    after: Vec<u8>,
    published: Option<Snapshot>,
    stage: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    id: String,
    status: String,
    context: Context,
    requests: Vec<ToggleRequest>,
    changes: Vec<Change>,
    undo_changes: Vec<Change>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    home: PathBuf,
    project: PathBuf,
}
impl From<&Paths> for Context {
    fn from(p: &Paths) -> Self {
        Self {
            home: p.home.clone(),
            project: p.project.clone(),
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct PlanView {
    pub schema_version: u32,
    pub status: String,
    pub action: String,
    pub operation_id: String,
    pub token: String,
    pub changes: Vec<FileChangeView>,
    pub targets: Vec<ToggleRequest>,
    pub files: Vec<PathBuf>,
    pub notes: Vec<&'static str>,
}
#[derive(Debug, Clone, Serialize)]
pub struct FileChangeView {
    pub path: PathBuf,
    pub action: &'static str,
}
/// Contains private snapshots; deliberately has no Debug or Serialize implementation.
pub struct Plan {
    record: Record,
    view: PlanView,
    undo: bool,
    record_before: Option<Snapshot>,
    inventory_revision: Option<String>,
    preconditions: Vec<Change>,
}
impl Plan {
    pub fn view(&self) -> &PlanView {
        &self.view
    }
}
#[derive(Serialize)]
pub struct OperationView {
    pub operation_id: String,
    pub status: String,
    pub targets: Vec<ToggleRequest>,
    pub changed_files: usize,
    pub recovery: &'static str,
}
#[derive(Serialize)]
pub struct History {
    pub schema_version: u32,
    pub operations: Vec<OperationView>,
}
struct Journal {
    path: PathBuf,
    dir: Directory,
    _lock: File,
}
impl Drop for Journal {
    fn drop(&mut self) {
        // Explicit unlock also releases descriptions briefly inherited by an unrelated fork.
        let _ = self._lock.unlock();
    }
}
impl Journal {
    fn open(paths: &Paths) -> Result<Self, AppError> {
        let state = Directory::open(&paths.state_dir)?;
        let dir = state.private_child("journal")?;
        let lock = dir.lock()?;
        let journal = Self {
            path: paths.state_dir.join("journal"),
            dir,
            _lock: lock,
        };
        journal.check()?;
        Ok(journal)
    }
    fn check(&self) -> Result<(), AppError> {
        let current = Directory::open(&self.path)?;
        if current.chain != self.dir.chain {
            return Err(conflict());
        }
        let m = std::fs::symlink_metadata(&self.path).map_err(|_| journal_error())?;
        use std::os::unix::fs::PermissionsExt;
        if m.permissions().mode() & 0o777 != 0o700 {
            return Err(journal_error());
        }
        // Lock alias/replacement after acquisition must not split cooperating processes.
        if self.dir.read("lock", true)?.is_none() {
            return Err(journal_error());
        }
        use std::os::unix::fs::MetadataExt;
        let held = self._lock.metadata().map_err(|_| journal_error())?;
        let current =
            std::fs::symlink_metadata(self.path.join("lock")).map_err(|_| journal_error())?;
        if held.dev() != current.dev() || held.ino() != current.ino() || held.nlink() != 1 {
            return Err(journal_error());
        }
        Ok(())
    }
    fn load(&self, id: &str) -> Result<(Record, Snapshot), AppError> {
        if !valid_id(id) {
            return Err(AppError::new(
                "invalid_arguments",
                "Operation IDs have the form op- followed by 32 lowercase hex digits.",
            ));
        }
        self.check()?;
        let bytes = self
            .dir
            .read(&format!("{id}.json"), true)?
            .ok_or(AppError::new(
                "item_not_found",
                "Operation ID was not found in this private journal.",
            ))?;
        let record = journal_record::decode(&bytes.bytes)?;
        if ![1, 2].contains(&record.version)
            || record.id != id
            || !["prepared", "applied", "undoing", "undone"].contains(&record.status.as_str())
            || record.changes.is_empty()
            || record.changes.len() > 100
            || record.requests.is_empty()
            || record.requests.len() > 100
            || record.requests.iter().any(|r| {
                r.validate().is_err()
                    || !r.id.starts_with("occurrence-")
                    || !r.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        {
            return Err(journal_error());
        }
        Ok((record, bytes))
    }
    fn save(&self, record: &Record, expected: Option<&Snapshot>) -> Result<Snapshot, AppError> {
        self.check()?;
        let name = format!("{}.json", record.id);
        if self.dir.read(&name, true)?.as_ref() != expected {
            return Err(conflict());
        }
        let bytes = journal_record::encode(record)?;
        if bytes.len() > journal_record::LIMIT {
            return Err(refused(
                "Operation exceeds the 4 MiB private record budget; select fewer targets.",
            ));
        }
        let stage = format!("{}.record", random_id()?);
        self.dir.create(&stage, &bytes, 0o600, None)?;
        self.check()?;
        if self.dir.read(&name, true)?.as_ref() != expected {
            return Err(conflict());
        }
        self.dir.replace(&stage, &name, expected.is_none())?;
        self.dir.read(&name, true)?.ok_or_else(journal_error)
    }
    fn records(&self) -> Result<Vec<Record>, AppError> {
        self.check()?;
        let mut ids = vec![];
        for entry in std::fs::read_dir(&self.path).map_err(|_| journal_error())? {
            let entry = entry.map_err(|_| journal_error())?;
            let name = entry.file_name();
            let name = name.to_str().ok_or_else(journal_error)?;
            if let Some(id) = name.strip_suffix(".json") {
                if !valid_id(id) {
                    return Err(journal_error());
                }
                ids.push(id.to_string());
            }
            if ids.len() > 10000 {
                return Err(journal_error());
            }
        }
        ids.sort();
        let mut total = 0usize;
        let mut records = vec![];
        for id in ids {
            let (record, snapshot) = self.load(&id)?;
            total += snapshot.bytes.len();
            if total > 64 * 1024 * 1024 {
                return Err(refused(
                    "History exceeds the 64 MiB read budget; retain the journal and use an exact operation ID for recovery.",
                ));
            }
            records.push(record);
        }
        Ok(records)
    }
}
fn scope(s: &str) -> Scope {
    match s {
        "user" => Scope::User,
        "project" => Scope::Project,
        _ => Scope::Local,
    }
}
fn agent(s: &str) -> Agent {
    if s == "claude" {
        Agent::Claude
    } else {
        Agent::Codex
    }
}
fn text(snapshot: &Option<Snapshot>, default: &str) -> Result<String, AppError> {
    snapshot
        .as_ref()
        .map(|s| {
            String::from_utf8(s.bytes.clone()).map_err(|_| {
                refused("Configuration must be supported UTF-8; raw parser details are withheld.")
            })
        })
        .unwrap_or_else(|| Ok(default.into()))
}
fn change(path: PathBuf, id: &str, index: usize) -> Result<Change, AppError> {
    let dir = Directory::open(path.parent().ok_or_else(journal_error)?)?;
    let before = dir.read(filename(&path)?, false)?;
    Ok(Change {
        path,
        parents: dir.chain,
        before,
        after: vec![],
        published: None,
        stage: format!(".skillrail-{id}-{index}"),
    })
}
fn check_change(c: &Change, expected: &Option<Snapshot>) -> Result<Directory, AppError> {
    let d = Directory::open(c.path.parent().ok_or_else(journal_error)?)?;
    if d.chain != c.parents || d.read(filename(&c.path)?, false)? != *expected {
        return Err(conflict());
    }
    Ok(d)
}
fn revision(inventory: crate::Inventory) -> Result<String, AppError> {
    Ok(
        crate::query::query_snapshot(inventory, &crate::query::Query::default())?
            .page
            .revision,
    )
}
impl Application {
    pub fn plan_toggle(&self, request: ToggleRequest) -> Result<Plan, AppError> {
        self.plan_toggles(vec![request])
    }
    /// Batch seam for recoverable changes to several files. No filesystem transaction claim.
    pub fn plan_toggles(&self, requests: Vec<ToggleRequest>) -> Result<Plan, AppError> {
        if requests.is_empty() || requests.len() > 100 {
            return Err(AppError::new(
                "invalid_arguments",
                "Supply 1..100 explicit toggle targets.",
            ));
        }
        for r in &requests {
            r.validate()?;
        }
        let inventory = crate::discovery::discover(&self.paths, 0);
        let op = random_id()?;
        let mut changes: Vec<Change> = vec![];
        let mut normalized = vec![];
        let mut consulted = vec![];
        for request in requests {
            let candidates: Vec<_> = inventory
                .occurrences
                .iter()
                .filter(|o| {
                    (o.id == request.id || o.item_id == request.id)
                        && o.agent == agent(&request.agent)
                        && o.scope == scope(&request.scope)
                })
                .collect();
            if candidates.is_empty() {
                return Err(AppError::new(
                    "item_not_found",
                    "No occurrence matches this ID, agent and scope.",
                ));
            }
            if candidates.len() != 1 {
                return Err(refused(
                    "Several occurrences match; pass the exact occurrence ID shown by scan/show.",
                ));
            }
            let o = candidates[0];
            if !matches!(o.origin, Origin::Standalone) {
                return Err(refused(
                    "Plugin-owned visibility and lifecycle belong to the native agent; no plugin files or activation settings were changed.",
                ));
            }
            if o.agent == Agent::Codex && o.mcp.is_none() && o.scope != Scope::User {
                return Err(refused(
                    "Codex native skills.config reads only User and SessionFlags layers; project-scoped skill toggles are unsupported. No user-wide fallback was written.",
                ));
            }
            if !o.diagnostics.is_empty()
                || o.mcp.as_ref().is_some_and(|m| {
                    matches!(
                        m.configured_status,
                        ConfiguredStatus::Unknown | ConfiguredStatus::Invalid
                    )
                })
            {
                return Err(refused(
                    "Target schema or metadata is uncertain; resolve its inventory diagnostics before toggling.",
                ));
            }
            // Native selectors may address multiple physical occurrences. Never silently widen a selection.
            let same = inventory
                .occurrences
                .iter()
                .filter(|other| {
                    other.agent == o.agent
                        && other.mcp.is_some() == o.mcp.is_some()
                        && if o.agent == Agent::Claude || o.mcp.is_some() {
                            other.name == o.name
                        } else {
                            other.canonical_path == o.canonical_path
                        }
                })
                .count();
            if same > 1 {
                return Err(refused(
                    "The native selector also addresses another occurrence or scope; use the native agent to resolve the collision. Shared sources and links were preserved.",
                ));
            }
            let path = if o.agent == Agent::Codex {
                if o.mcp.is_some() {
                    o.original_path.clone()
                } else {
                    // The native skill-rule resolver ignores project configuration layers.
                    self.paths.home.join(".codex/config.toml")
                }
            } else if o.mcp.is_some() {
                self.paths.home.join(".claude.json")
            } else if o.scope == Scope::User {
                self.paths.home.join(".claude/settings.json")
            } else {
                self.paths.project.join(".claude/settings.json")
            };
            let index = if let Some(i) = changes.iter().position(|c| c.path == path) {
                i
            } else {
                changes.push(change(path, &op, changes.len())?);
                changes.len() - 1
            };
            let c = &mut changes[index];
            let input = if c.after.is_empty() {
                text(&c.before, if o.agent == Agent::Codex { "" } else { "{}\n" })?
            } else {
                String::from_utf8(c.after.clone()).map_err(|_| journal_error())?
            };
            let patched = if o.agent == Agent::Codex {
                if o.mcp.is_some() {
                    config_patch::toml_mcp(&input, &o.name, request.enabled)?
                } else {
                    validate_skills(&input)?;
                    let skill = o
                        .canonical_path
                        .as_ref()
                        .ok_or_else(|| {
                            refused(
                                "Canonical source is unavailable; no source or links were changed.",
                            )
                        })?
                        .join("SKILL.md");
                    config_patch::toml_skill(
                        &input,
                        skill.to_str().ok_or_else(journal_error)?,
                        &o.name,
                        request.enabled,
                    )?
                }
            } else if o.mcp.is_some() {
                if !o
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                    || ["workspace", "computer-use", "claude-in-chrome"].contains(&o.name.as_str())
                {
                    return Err(refused(
                        "Unsupported or reserved native MCP selector; no configuration was changed.",
                    ));
                }
                let v = crate::json_config::parse(&input)
                    .map_err(|_| refused("Claude configuration is ambiguous or malformed."))?;
                let project = self.paths.project.to_str().ok_or_else(journal_error)?;
                let list = v
                    .get("projects")
                    .and_then(|v| v.get(project))
                    .and_then(|v| v.get("disabledMcpServers"));
                let mut names: Vec<String> = match list {
                    None => vec![],
                    Some(v) => v
                        .as_array()
                        .ok_or_else(|| refused("Unsupported configuration list; use the native agent to repair it."))?
                        .iter()
                        .map(|v| v.as_str().map(str::to_owned).ok_or_else(|| refused("Unsupported configuration list entry; raw values were withheld.")))
                        .collect::<Result<_, _>>()?,
                };
                if request.enabled {
                    names.retain(|n| n != &o.name);
                } else if !names.contains(&o.name) {
                    names.push(o.name.clone());
                }
                if list == Some(&serde_json::json!(names)) || (list.is_none() && request.enabled) {
                    input.clone()
                } else {
                    config_patch::json_set(
                        &input,
                        &["projects", project, "disabledMcpServers"],
                        serde_json::json!(names),
                    )?
                }
            } else {
                let v = crate::json_config::parse(&input)
                    .map_err(|_| refused("Claude settings are ambiguous or malformed."))?;
                if v.get("skillOverrides").is_some_and(|v| {
                    !v.as_object().is_some_and(|m| {
                        m.values().all(|v| {
                            matches!(
                                v.as_str(),
                                Some("on" | "off" | "name-only" | "user-invocable-only")
                            )
                        })
                    })
                }) {
                    return Err(refused(
                        "Unsupported Claude skillOverrides schema; no settings changed.",
                    ));
                }
                if !v.is_object() {
                    return Err(refused(
                        "Claude settings must be an object; no visibility state was inferred.",
                    ));
                }
                let desired = if request.enabled { "on" } else { "off" };
                let local = v
                    .get("skillOverrides")
                    .and_then(|v| v.get(&o.name))
                    .and_then(serde_json::Value::as_str);
                let default_on = if request.enabled && o.scope == Scope::Project {
                    claude_project_default_on(&self.paths, &o.name, &c.path, &mut consulted, &op)?
                } else {
                    true
                };
                if local == Some(desired) || (local.is_none() && request.enabled && default_on) {
                    input.clone()
                } else {
                    config_patch::json_set(
                        &input,
                        &["skillOverrides", &o.name],
                        serde_json::json!(desired),
                    )?
                }
            };
            c.after = if c.before.is_none() && patched == input {
                vec![]
            } else {
                patched.into_bytes()
            };
            normalized.push(ToggleRequest {
                id: o.id.clone(),
                ..request
            });
        }
        let mut preconditions = changes.clone();
        preconditions.extend(consulted);
        changes.retain(|c| {
            (c.before.is_some() || !c.after.is_empty())
                && c.before.as_ref().map(|s| s.bytes.as_slice()) != Some(c.after.as_slice())
        });
        let inventory_revision = Some(revision(inventory)?);
        let record = Record {
            version: 2,
            id: op,
            status: "prepared".into(),
            context: (&self.paths).into(),
            requests: normalized,
            changes,
            undo_changes: vec![],
        };
        journal_record::admit(&record)?;
        make_plan(record, false, None, inventory_revision, preconditions)
    }
    pub fn operations(&self) -> Result<History, AppError> {
        match std::fs::symlink_metadata(self.paths.state_dir.join("journal")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(History {
                    schema_version: 1,
                    operations: vec![],
                });
            }
            Err(_) => return Err(journal_error()),
            Ok(_) => {}
        }
        let j = Journal::open(&self.paths)?;
        let operations = j.records()?.into_iter().map(|r| OperationView { operation_id: r.id, status: r.status.clone(), targets: r.requests, changed_files: r.changes.len(), recovery: if r.status == "prepared" || r.status == "undoing" { "Interrupted operation: preview undo to restore only matching published files; resolve conflicts manually without deleting the journal." } else { "Undo requires exact postconditions; intervening edits are preserved." } }).collect();
        Ok(History {
            schema_version: 1,
            operations,
        })
    }
    pub fn plan_undo(&self, id: &str) -> Result<Plan, AppError> {
        if !valid_id(id) {
            return Err(AppError::new(
                "invalid_arguments",
                "Supply a valid operation ID from operations.",
            ));
        }
        let j = Journal::open(&self.paths)?;
        let (r, before) = j.load(id)?;
        if r.context != Context::from(&self.paths) {
            return Err(refused(
                "Operation belongs to another home/project context; use its original explicit paths.",
            ));
        }
        if r.status != "undone" {
            // Validate the complete set before restoring any file.
            for c in &r.changes {
                let dir = Directory::open(c.path.parent().ok_or_else(journal_error)?)?;
                if dir.chain != c.parents {
                    return Err(conflict());
                }
                let current = dir.read(filename(&c.path)?, false)?;
                let restored = r
                    .undo_changes
                    .iter()
                    .find(|u| u.path == c.path)
                    .and_then(|u| u.published.as_ref());
                if current != c.before
                    && current.as_ref() != c.published.as_ref()
                    && current.as_ref() != restored
                {
                    return Err(conflict());
                }
                if current.is_none() && c.before.is_some() {
                    return Err(conflict());
                }
            }
        }
        make_plan(r, true, Some(before), None, vec![])
    }
    fn revalidate_forward(&self, plan: &Plan) -> Result<(), AppError> {
        if plan.inventory_revision.as_ref()
            != Some(&revision(crate::discovery::discover(&self.paths, 0))?)
        {
            return Err(conflict());
        }
        for c in &plan.preconditions {
            check_change(c, &c.before)?;
        }
        Ok(())
    }
    pub fn apply(&self, plan: &Plan) -> Result<PlanView, AppError> {
        self.apply_with_checkpoint(plan, |_| Ok(()))
    }
    /// Deterministic interruption/race seam; no environment flag or production bypass.
    /// A checkpoint error leaves the same durable recovery state as process termination.
    pub fn apply_with_checkpoint(
        &self,
        plan: &Plan,
        mut checkpoint: impl FnMut(Checkpoint) -> Result<(), AppError>,
    ) -> Result<PlanView, AppError> {
        if plan.record.context != Context::from(&self.paths) {
            return Err(refused("Plan belongs to another application context."));
        }
        let mut view = plan.view.clone();
        if plan.record.changes.is_empty() || (plan.undo && plan.record.status == "undone") {
            if let Some(expected) = &plan.inventory_revision
                && expected != &revision(crate::discovery::discover(&self.paths, 0))?
            {
                return Err(conflict());
            }
            for c in &plan.preconditions {
                check_change(c, &c.before)?;
            }
            view.status = "unchanged".into();
            return Ok(view);
        }
        // Reject an already-stale selection before creating a private journal/lock.
        // These optimistic checks are repeated under the application lock below.
        if !plan.undo {
            self.revalidate_forward(plan)?;
        }
        let j = Journal::open(&self.paths)?;
        checkpoint(Checkpoint::Locked)?;
        let mut r = plan.record.clone();
        let mut record_snapshot;
        if plan.undo {
            let (_, current) = j.load(&r.id)?;
            if Some(&current) != plan.record_before.as_ref() {
                return Err(conflict());
            }
            if r.status != "undoing" {
                // Plan restoration once; retain every staged/restored inode across retries.
                for c in &r.changes {
                    let d = Directory::open(c.path.parent().ok_or_else(journal_error)?)?;
                    if d.chain != c.parents {
                        return Err(conflict());
                    }
                    let current = d.read(filename(&c.path)?, false)?;
                    if current == c.before {
                        continue;
                    }
                    if current.as_ref() != c.published.as_ref() || current.is_none() {
                        return Err(conflict());
                    }
                    let mut undo = change(c.path.clone(), &r.id, r.undo_changes.len())?;
                    undo.stage.push_str("-undo");
                    undo.after = c
                        .before
                        .as_ref()
                        .map(|s| s.bytes.clone())
                        .unwrap_or_default();
                    r.undo_changes.push(undo);
                }
            }
            for u in &r.undo_changes {
                let d = Directory::open(u.path.parent().ok_or_else(journal_error)?)?;
                if d.chain != u.parents {
                    return Err(conflict());
                }
                let current = d.read(filename(&u.path)?, false)?;
                let delete = r
                    .changes
                    .iter()
                    .find(|c| c.path == u.path)
                    .is_some_and(|c| c.before.is_none());
                if current != u.before
                    && !(u.published.is_some() && current == u.published)
                    && !(delete && current.is_none())
                {
                    return Err(conflict());
                }
            }
            r.status = "undoing".into();
            record_snapshot = j.save(&r, Some(&current))?;
        } else {
            if let Ok((existing, _)) = j.load(&r.id) {
                if existing.status == "applied" {
                    view.status = "applied".into();
                    return Ok(view);
                }
                return Err(refused(
                    "Operation already has a recovery record; inspect operations and undo before retrying.",
                ));
            }
            if j.records()?
                .iter()
                .any(|r| r.status == "prepared" || r.status == "undoing")
            {
                return Err(refused(
                    "An interrupted operation requires recovery; inspect operations and preview undo before applying new changes.",
                ));
            }
            self.revalidate_forward(plan)?;
            record_snapshot = j.save(&r, None)?;
        }
        checkpoint(Checkpoint::Recorded)?;
        let len = if plan.undo {
            r.undo_changes.len()
        } else {
            r.changes.len()
        };
        for i in 0..len {
            let c = if plan.undo {
                &r.undo_changes[i]
            } else {
                &r.changes[i]
            };
            let delete = plan.undo
                && r.changes
                    .iter()
                    .find(|orig| orig.path == c.path)
                    .is_some_and(|orig| orig.before.is_none());
            let existing_dir = Directory::open(c.path.parent().ok_or_else(journal_error)?)?;
            if existing_dir.chain != c.parents {
                return Err(conflict());
            }
            let current = existing_dir.read(filename(&c.path)?, false)?;
            if plan.undo
                && ((delete && current.is_none())
                    || (c.published.is_some() && current == c.published))
            {
                continue;
            }
            let dir = check_change(c, &c.before)?;
            if !delete {
                let staging = dir.private_child(&c.stage)?;
                let existing = staging.read("replacement", false)?;
                if let Some(stage) = &existing {
                    if !plan.undo
                        || stage.bytes != c.after
                        || c.published.as_ref().is_some_and(|p| p != stage)
                    {
                        return Err(conflict());
                    }
                } else if c.published.is_some() {
                    // A recorded stage cannot disappear without an observed publication/edit.
                    return Err(conflict());
                }
                let mode = if plan.undo {
                    r.changes
                        .iter()
                        .find(|orig| orig.path == c.path)
                        .and_then(|c| c.before.as_ref())
                        .map(Snapshot::mode)
                        .unwrap_or(0o600)
                } else {
                    c.before.as_ref().map(Snapshot::mode).unwrap_or(0o600)
                };
                let group = if plan.undo {
                    r.changes
                        .iter()
                        .find(|original| original.path == c.path)
                        .and_then(|original| original.before.as_ref())
                        .map(Snapshot::group)
                } else {
                    c.before.as_ref().map(Snapshot::group)
                };
                let staged = if c.published.is_some() {
                    // Reuse the durable inode. Recreating it before saving new evidence can
                    // strand recovery if the process dies during the replacement's fsync.
                    existing.ok_or_else(conflict)?
                } else {
                    // No identity was committed yet; an interrupted initial stage can be rebuilt.
                    if existing.is_some() {
                        staging.remove("replacement")?;
                    }
                    staging.create("replacement", &c.after, mode, group)?;
                    staging
                        .read("replacement", false)?
                        .ok_or_else(journal_error)?
                };
                if plan.undo {
                    r.undo_changes[i].published = Some(staged);
                } else {
                    r.changes[i].published = Some(staged);
                }
                record_snapshot = j.save(&r, Some(&record_snapshot))?;
            }
            checkpoint(Checkpoint::Staged(i))?;
            let c = if plan.undo {
                &r.undo_changes[i]
            } else {
                &r.changes[i]
            };
            j.check()?;
            let dir = check_change(c, &c.before)?;
            if delete {
                dir.remove(filename(&c.path)?)?;
            } else {
                let staging = dir.private_child(&c.stage)?;
                if staging.read("replacement", false)? != c.published {
                    return Err(conflict());
                }
                checkpoint(Checkpoint::BeforePublish(i))?;
                staging.replace_into(
                    "replacement",
                    &dir,
                    filename(&c.path)?,
                    c.before.is_none(),
                )?;
                dir.remove_dir(&c.stage)?;
            }
            checkpoint(Checkpoint::Published(i))?;
            // Revalidate path binding after publication; recovery retains exact stage identity.
            let reopened = Directory::open(c.path.parent().ok_or_else(journal_error)?)?;
            if reopened.chain != c.parents
                || reopened.read(filename(&c.path)?, false)?
                    != if delete { None } else { c.published.clone() }
            {
                return Err(conflict());
            }
        }
        if plan.undo {
            // Delete only our exact matching staged replacements left by an interruption.
            for c in &r.changes {
                let d = Directory::open(c.path.parent().ok_or_else(journal_error)?)?;
                if d.chain != c.parents {
                    return Err(conflict());
                }
                let staging = d.private_child(&c.stage)?;
                if let Some(stage) = staging.read("replacement", false)? {
                    if stage.bytes != c.after || c.published.as_ref().is_some_and(|p| p != &stage) {
                        return Err(conflict());
                    }
                    staging.remove("replacement")?;
                }
                d.remove_dir(&c.stage)?;
            }
            for c in &r.undo_changes {
                let d = Directory::open(c.path.parent().ok_or_else(journal_error)?)?;
                if d.chain != c.parents {
                    return Err(conflict());
                }
                let staging = d.private_child(&c.stage)?;
                if let Some(stage) = staging.read("replacement", false)? {
                    if stage.bytes != c.after || c.published.as_ref().is_some_and(|p| p != &stage) {
                        return Err(conflict());
                    }
                    staging.remove("replacement")?;
                }
                d.remove_dir(&c.stage)?;
            }
        }
        r.status = if plan.undo { "undone" } else { "applied" }.into();
        j.save(&r, Some(&record_snapshot))?;
        view.status = r.status;
        Ok(view)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checkpoint {
    Locked,
    Recorded,
    Staged(usize),
    Published(usize),
    BeforePublish(usize),
}
fn make_plan(
    record: Record,
    undo: bool,
    record_before: Option<Snapshot>,
    inventory_revision: Option<String>,
    preconditions: Vec<Change>,
) -> Result<Plan, AppError> {
    let facts: Vec<_> = preconditions
        .iter()
        .map(|c| (&c.path, &c.parents, &c.before, &c.after))
        .collect();
    let encoded = serde_json::to_string(&(
        &record.context,
        &record.requests,
        &facts,
        &inventory_revision,
        undo,
        &record_before,
    ))
    .map_err(|_| journal_error())?;
    let view = PlanView {
        schema_version: 1,
        status: "preview".into(),
        action: if undo { "undo" } else { "toggle" }.into(),
        changes: record
            .changes
            .iter()
            .map(|c| FileChangeView {
                path: c.path.clone(),
                action: if undo && c.before.is_none() {
                    "remove_created_file"
                } else if undo {
                    "restore_original_bytes"
                } else if c.before.is_none() {
                    "create_config"
                } else {
                    "replace_config"
                },
            })
            .collect(),
        operation_id: record.id.clone(),
        token: crate::files::id(&[&encoded]),
        targets: record.requests.clone(),
        files: record.changes.iter().map(|c| c.path.clone()).collect(),
        notes: vec![
            "Configuration changes only; runtime loading, trust, managed policy and session overrides remain unresolved.",
            "Existing owners, skill content, locations and links are preserved. Claude MCP opt-outs apply only to the selected project, including user definitions.",
            "Apply revalidates exact files and path bindings; multi-file changes are journaled and recoverable, not filesystem-atomic.",
        ],
    };
    Ok(Plan {
        record,
        view,
        undo,
        record_before,
        inventory_revision,
        preconditions,
    })
}
fn validate_skills(input: &str) -> Result<(), AppError> {
    let v: toml::Value =
        toml::from_str(input).map_err(|_| refused("Unsupported TOML configuration."))?;
    if let Some(config) = v.get("skills").and_then(|v| v.get("config")) {
        let rules = config
            .as_array()
            .ok_or_else(|| refused("Unsupported skills.config array schema."))?;
        for rule in rules {
            let map = rule
                .as_table()
                .ok_or_else(|| refused("Unsupported skill selector schema."))?;
            let path = map.get("path").and_then(toml::Value::as_str);
            let name = map.get("name").and_then(toml::Value::as_str);
            if map
                .keys()
                .any(|k| !["path", "name", "enabled"].contains(&k.as_str()))
                || path.is_some() == name.is_some()
                || name.is_some_and(|n| n.trim().is_empty())
                || map.get("enabled").and_then(toml::Value::as_bool).is_none()
                || path.is_some_and(|p| !Path::new(p).is_absolute())
            {
                return Err(refused(
                    "Unsupported or ambiguous existing skill selector; use the native agent to repair it.",
                ));
            }
        }
    }
    Ok(())
}

// Absence in a project file is inheritance, not an explicit native "on" rule.
fn claude_project_default_on(
    paths: &Paths,
    name: &str,
    target: &Path,
    consulted: &mut Vec<Change>,
    op: &str,
) -> Result<bool, AppError> {
    let mut configs = vec![(paths.home.join(".claude/settings.json"), false)];
    for root in crate::discovery::project_ancestors(&paths.project) {
        configs.push((root.join(".claude/settings.json"), false));
        configs.push((root.join(".claude/settings.local.json"), true));
    }
    let mut default_on = true;
    for (path, local) in configs {
        if path == target || crate::mcp_inventory::absent(&path) {
            continue;
        }
        let guard = change(path, op, 0)?;
        let input = text(&guard.before, "{}")?;
        let value = crate::json_config::parse(&input).map_err(|_| {
            refused(
                "Inherited Claude skill settings are uncertain; no default-on state was inferred.",
            )
        })?;
        if !value.is_object() || value.get("skillOverrides").is_some_and(|v| !v.is_object()) {
            return Err(refused(
                "Inherited Claude skill settings are unsupported; no default-on state was inferred.",
            ));
        }
        if let Some(state) = value.get("skillOverrides").and_then(|v| v.get(name)) {
            match state.as_str() {
                Some("on") => {}
                Some("off" | "name-only" | "user-invocable-only") if local => {
                    return Err(refused(
                        "A local Claude skill override restricts this name; a project setting cannot safely enable it. Change that override in the native agent.",
                    ));
                }
                Some("off" | "name-only" | "user-invocable-only") => default_on = false,
                _ => {
                    return Err(refused(
                        "Inherited Claude skill state is unsupported; no default-on state was inferred.",
                    ));
                }
            }
        }
        consulted.push(guard);
    }
    Ok(default_on)
}
