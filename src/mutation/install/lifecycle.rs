//! Owned lifecycle: retain original inodes privately; publish replacements with M5 guards.
use super::*;
mod recovery;
pub(in crate::mutation) use recovery::{apply, plan_undo};
pub(super) use recovery::{revalidate_source, validate_undo};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::mutation) struct Lifecycle {
    old: Install,
    backup: String,
    link_chains: Vec<Option<Vec<Identity>>>,
    noop: bool,
    source_chain: Option<Vec<Identity>>,
}
impl Lifecycle {
    pub(in crate::mutation) fn created_count(&self) -> usize {
        self.old.created_count()
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct ContentState {
    pub sha256: Option<String>,
    pub size: usize,
    pub mode: u32,
    pub kind: &'static str,
}
#[derive(Debug, Clone, Serialize)]
pub struct ContentDiff {
    pub path: String,
    pub action: &'static str,
    pub before: Option<ContentState>,
    pub after: Option<ContentState>,
}
fn state(e: &Entry) -> ContentState {
    use sha2::{Digest, Sha256};
    ContentState {
        sha256: (!e.directory).then(|| {
            Sha256::digest(&e.bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect()
        }),
        size: e.bytes.len(),
        mode: if e.directory || e.executable {
            0o700
        } else {
            0o600
        },
        kind: if e.directory { "directory" } else { "file" },
    }
}
fn diff(before: &[Entry], after: &[Entry]) -> Vec<ContentDiff> {
    let paths: BTreeSet<_> = before.iter().chain(after).map(|e| &e.path).collect();
    paths
        .into_iter()
        .filter_map(|path| {
            let b = before.iter().find(|e| &e.path == path);
            let a = after.iter().find(|e| &e.path == path);
            (b != a).then(|| ContentDiff {
                path: path.clone(),
                action: if b.is_none() {
                    "added"
                } else if a.is_none() {
                    "removed"
                } else {
                    "changed"
                },
                before: b.map(state),
                after: a.map(state),
            })
        })
        .collect()
}
fn old_record(r: &Record, old: Install) -> Record {
    Record {
        version: 3,
        id: old
            .stage
            .trim_start_matches(".skillrail-")
            .trim_end_matches("-install")
            .into(),
        context: r.context.clone(),
        status: "applied".into(),
        requests: vec![],
        changes: vec![],
        undo_changes: vec![],
        install: Some(old),
        lifecycle: None,
    }
}
pub(super) fn validate_record(r: &Record) -> Result<(), AppError> {
    let l = r.lifecycle.as_ref().ok_or_else(journal_error)?;
    if r.version != 4
        || l.backup != format!(".skillrail-{}-original", r.id)
        || l.link_chains.len() != l.old.links.len()
        || l.old.tree_chain.is_none()
        || l.old
            .links
            .iter()
            .any(|link| link.published.is_none() || link.parents.is_none())
        || !r.changes.is_empty()
        || !r.requests.is_empty()
        || !r.undo_changes.is_empty()
    {
        return Err(journal_error());
    }
    let old = old_record(r, l.old.clone());
    if !valid_id(&old.id) {
        return Err(journal_error());
    }
    super::validate_record(&old)?;
    if r.install.is_some() {
        let mut new = r.clone();
        new.version = 3;
        new.lifecycle = None;
        super::validate_record(&new)?;
        let i = r.install.as_ref().ok_or_else(journal_error)?;
        if i.manifest.name != l.old.manifest.name
            || i.manifest.links != l.old.manifest.links
            || i.request != l.old.request
            || i.journal != l.old.journal
        {
            return Err(journal_error());
        }
    }
    Ok(())
}
// Original links must be present, with exact durable identities, for a new mutation.
fn original(app: &Application, i: &Install) -> Result<(), AppError> {
    check_journal(app, i)?;
    i.store.check()?;
    check_tree(i, &i.manifest.snapshot, false)?;
    validate_consumers(app, i)?;
    check_names(app, i)?;
    for l in &i.links {
        if link_dir(l)?
            .ok_or_else(conflict)?
            .link(filename(&l.path)?)?
            != l.published
            || l.published.is_none()
        {
            return Err(conflict());
        }
    }
    Ok(())
}
fn check_journal(app: &Application, i: &Install) -> Result<(), AppError> {
    if i.journal != app.paths.state_dir {
        return Err(refused(
            "Install belongs to another private journal; use its original --state-dir.",
        ));
    }
    Ok(())
}
impl Application {
    pub fn plan_update(&self, id: &str) -> Result<Plan, AppError> {
        self.plan_owned(id, true)
    }
    pub fn plan_uninstall(&self, id: &str) -> Result<Plan, AppError> {
        self.plan_owned(id, false)
    }
    fn plan_owned(&self, id: &str, update: bool) -> Result<Plan, AppError> {
        let inventory = crate::discovery::discover(&self.paths, 0);
        let mut snapshots = BTreeSet::new();
        for o in &inventory.occurrences {
            if o.id == id || o.item_id == id {
                if !matches!(o.origin, Origin::Standalone) {
                    return Err(refused("Plugin lifecycle belongs to its native owner."));
                }
                snapshots.insert(o.canonical_path.clone().ok_or_else(conflict)?);
            }
        }
        if id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
            snapshots.insert(self.paths.home.join(".skillrail-sources").join(id));
        }
        if snapshots.len() > 1 {
            return Err(refused(
                "Several owned snapshots match; select one exact owned or occurrence ID.",
            ));
        }
        if snapshots.is_empty() {
            return Err(AppError::new(
                "item_not_found",
                "Select one exact owned/item/occurrence ID from inventory.",
            ));
        }
        let snapshot = snapshots.into_iter().next().ok_or_else(conflict)?;
        if absent(&snapshot) {
            return Err(AppError::new(
                "item_not_found",
                "Owned snapshot ID was not found.",
            ));
        }
        if owned_source(&self.paths, &snapshot).is_none() {
            return Err(refused(
                "A verified Skillrail-owned manifest is required; external content was preserved.",
            ));
        }
        let j = Journal::open(&self.paths)?;
        let records = j.records()?;
        let mut old = None;
        for r in records
            .iter()
            .filter(|r| r.status == "applied" && r.context == Context::from(&self.paths))
        {
            if let Some(i) = &r.install
                && i.manifest.snapshot == snapshot
                && original(self, i).is_ok()
            {
                old = Some(i.clone());
            }
        }
        let mut old = old.ok_or_else(|| refused("Owned content or links differ from durable installation evidence; preserve edits and use the original journal/context."))?;
        original(self, &old)?;
        // Pin selection to the manifest, even if an originally unambiguous source grows another skill.
        // Provenance normalizes remote identity (including a .git suffix). Keep
        // the declared HTTPS endpoint; normalize only local paths for later cwd use.
        if !old.request.source.starts_with("https://") {
            old.request.source = old.manifest.provenance.origin.clone();
        }
        old.request.skill = Some(old.manifest.provenance.subpath.clone());
        old.request.revision = old.manifest.provenance.reference.clone();
        let mut r = if update {
            self.prepare_install(old.request.clone())?
        } else {
            Record {
                version: 4,
                id: random_id()?,
                status: "prepared".into(),
                context: (&self.paths).into(),
                requests: vec![],
                changes: vec![],
                undo_changes: vec![],
                install: None,
                lifecycle: None,
            }
        };
        if let Some(i) = &r.install {
            if i.manifest.name != old.manifest.name {
                return Err(refused(
                    "Source name changed; uninstall and explicitly install the new name instead.",
                ));
            }
            if i.manifest.snapshot != snapshot && !absent(&i.manifest.snapshot) {
                return Err(conflict());
            }
        }
        let noop = r.install.as_ref().is_some_and(|i| {
            i.entries == old.entries && i.manifest.provenance == old.manifest.provenance
        });
        let source_chain = if update { source_chain(&old)? } else { None };
        r.version = 4;
        r.lifecycle = Some(Lifecycle {
            backup: format!(".skillrail-{}-original", r.id),
            link_chains: vec![None; old.links.len()],
            old,
            noop,
            source_chain,
        });
        if noop {
            validate_record(&r)?;
        } else {
            reserve_links(r.lifecycle.as_ref().ok_or_else(journal_error)?, update)?;
            admit(&r)?;
        }
        plan(r, false, None)
    }
}
pub(super) fn plan(r: Record, undo: bool, before: Option<Snapshot>) -> Result<Plan, AppError> {
    let l = r.lifecycle.as_ref().ok_or_else(journal_error)?;
    let encoded = serde_json::to_string(&(
        &r.context,
        &l.old,
        &l.source_chain,
        &r.install.as_ref().map(|i| (&i.request, &i.manifest)),
        undo,
        &before,
    ))
    .map_err(|_| journal_error())?;
    let mut files = vec![l.old.manifest.snapshot.clone()];
    files.extend(l.old.manifest.links.clone());
    if let Some(i) = &r.install
        && !files.contains(&i.manifest.snapshot)
    {
        files.push(i.manifest.snapshot.clone());
    }
    let after = r.install.as_ref().map_or(&[][..], |i| i.entries.as_slice());
    if l.noop {
        files.clear();
    }
    let view = PlanView {
        schema_version: 1,
        status: "preview".into(),
        action: if undo {
            "undo"
        } else if r.install.is_some() {
            "update"
        } else {
            "uninstall"
        }
        .into(),
        operation_id: r.id.clone(),
        token: crate::files::id(&[&encoded]),
        changes: files
            .iter()
            .map(|p| FileChangeView {
                path: p.clone(),
                action: if p == &l.old.manifest.snapshot {
                    if undo {
                        "restore_original_snapshot"
                    } else {
                        "retain_original_snapshot"
                    }
                } else if l.old.manifest.links.contains(p) {
                    if undo {
                        "restore_original_link"
                    } else if r.install.is_some() {
                        "retarget_owned_link"
                    } else {
                        "retain_owned_link"
                    }
                } else if undo {
                    "remove_replacement_snapshot"
                } else {
                    "create_updated_snapshot"
                },
            })
            .collect(),
        files,
        targets: vec![],
        content_diff: if undo {
            diff(after, &l.old.entries)
        } else {
            diff(&l.old.entries, after)
        },
        install: Some(r.install.as_ref().unwrap_or(&l.old).manifest.clone()),
        previous_install: Some(l.old.manifest.clone()),
        notes: vec![
            "Equal source content and provenance is a revalidated no-op: no files, links or operation record are published.",
            "Diffs contain relative paths, SHA-256, sizes, normalized modes and types only; file payloads are private.",
            "Original snapshots and links are retained privately with exact identities until recovery no longer needs them. Unknown consumers and edits refuse; arbitrary external filesystem references remain outside the selected context.",
            "Each rename is guarded and durable; the multi-path operation is recoverable, not filesystem-atomic.",
        ],
    };
    Ok(Plan {
        record: r,
        view,
        undo,
        record_before: before,
        inventory_revision: None,
        preconditions: vec![],
    })
}
// Reserve backup and replacement stages as well as the published link.
fn reserve_links(l: &Lifecycle, update: bool) -> Result<(), AppError> {
    if !absent(&l.old.store.path.join(&l.backup)) {
        return Err(conflict());
    }
    for link in &l.old.links {
        if !absent(&link.root.path.join(&l.backup)) {
            return Err(conflict());
        }
        let dir = link_dir(link)?.ok_or_else(conflict)?;
        let count = fs::read_dir(&link.root.path)
            .map_err(|_| conflict())?
            .take(10_001)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| conflict())?
            .len();
        if link_dir(link)?.ok_or_else(conflict)?.chain != dir.chain
            || count + if update { 2 } else { 1 } > 10_000
        {
            return Err(refused(
                "Selected root lacks complete lifecycle capacity for original backups, stages and links; no mutation was admitted.",
            ));
        }
    }
    Ok(())
}
fn admit(r: &Record) -> Result<(), AppError> {
    validate_record(r)?;
    let mut worst = r.clone();
    let max = Snapshot::reservation(&[]).identity;
    let l = worst.lifecycle.as_mut().ok_or_else(journal_error)?;
    for (chain, link) in l.link_chains.iter_mut().zip(&l.old.links) {
        *chain = Some(vec![max.clone(); link.root.path.components().count() + 2]);
    }
    if let Some(i) = &mut worst.install {
        i.tree_chain = Some(vec![max.clone(); i.store.path.components().count() + 2]);
        for e in expected(i)? {
            i.files.insert(e.path, max.clone());
        }
        for dir in directory_names(&expected(i)?) {
            i.files
                .insert(dir.to_string_lossy().into_owned(), max.clone());
        }
        for l in &mut i.links {
            l.parents = Some(vec![max.clone(); l.path.components().count()]);
            l.published = Some(Snapshot::reservation(
                i.manifest.snapshot.to_string_lossy().as_bytes(),
            ));
        }
    }
    if journal_record::encode(&worst)?.len() > journal_record::LIMIT {
        return Err(refused(
            "Complete owned lifecycle exceeds the 4 MiB private record budget; no mutation was admitted.",
        ));
    }
    Ok(())
}
fn source_chain(i: &Install) -> Result<Option<Vec<Identity>>, AppError> {
    let p = &i.manifest.provenance;
    if p.origin.starts_with("https://") {
        return Ok(None);
    }
    let path = if p.kind == "local" && p.subpath != "." {
        Path::new(&p.origin).join(&p.subpath)
    } else {
        PathBuf::from(&p.origin)
    };
    Ok(Some(Directory::open(&path)?.chain))
}
