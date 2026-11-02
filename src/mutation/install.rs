//! Owned tree and selected links, sharing the mutation journal, lock and plan lifecycle.
use super::*;
mod runtime;
use crate::install_source::{self, Entry, InstallRequest, Provenance};
pub(super) use runtime::apply;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Root {
    path: PathBuf,
    anchor: PathBuf,
    chain: Vec<Identity>,
}
impl Root {
    fn preview(path: PathBuf) -> Result<Self, AppError> {
        let mut anchor = path.clone();
        loop {
            match fs::symlink_metadata(&anchor) {
                Ok(_) => break,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    anchor = anchor.parent().ok_or_else(journal_error)?.into();
                }
                Err(_) => return Err(conflict()),
            }
        }
        let chain = Directory::open(&anchor)?.chain;
        Ok(Self {
            path,
            anchor,
            chain,
        })
    }
    fn check(&self) -> Result<(), AppError> {
        if Directory::open(&self.anchor)?.chain != self.chain {
            return Err(conflict());
        }
        Ok(())
    }
    fn ensure(&self) -> Result<Directory, AppError> {
        self.check()?;
        let mut d = Directory::open(&self.anchor)?;
        let mut p = self.anchor.clone();
        for part in self
            .path
            .strip_prefix(&self.anchor)
            .map_err(|_| journal_error())?
            .components()
        {
            let name = part.as_os_str().to_str().ok_or_else(journal_error)?;
            p.push(name);
            d = match fs::symlink_metadata(&p) {
                Ok(_) => Directory::open(&p)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => d.private_child(name)?,
                Err(_) => return Err(conflict()),
            };
        }
        Ok(d)
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Link {
    path: PathBuf,
    root: Root,
    parents: Option<Vec<Identity>>,
    published: Option<Snapshot>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Install {
    request: InstallRequest,
    journal: PathBuf,
    manifest: Manifest,
    entries: Vec<Entry>,
    store: Root,
    stage: String,
    tree_chain: Option<Vec<Identity>>,
    files: BTreeMap<String, Identity>,
    links: Vec<Link>,
}
impl Install {
    pub(super) fn created_count(&self) -> usize {
        self.entries.len() + self.links.len() + 1
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub version: u32,
    pub owned_id: String,
    pub name: String,
    pub provenance: Provenance,
    pub snapshot: PathBuf,
    pub links: Vec<PathBuf>,
    pub files: Vec<String>,
}
pub(super) fn validate_record(r: &Record) -> Result<(), AppError> {
    let Some(i) = &r.install else {
        return if r.version == 3 {
            Err(journal_error())
        } else {
            Ok(())
        };
    };
    if r.version != 3
        || !r.changes.is_empty()
        || !r.undo_changes.is_empty()
        || !r.requests.is_empty()
        || !i.manifest.provenance.valid()
        || i.entries.is_empty()
        || i.entries.len() > 1000
        || i.manifest.provenance.content_hash != install_source::hash(&i.entries)
        || i.manifest.owned_id
            != crate::files::id(&[
                "owned",
                &serde_json::to_string(&i.manifest.provenance).map_err(|_| journal_error())?,
            ])
        || i.manifest.snapshot
            != r.context
                .home
                .join(".skillrail-sources")
                .join(&i.manifest.owned_id)
        || i.store.path != r.context.home.join(".skillrail-sources")
        || i.stage != format!(".skillrail-{}-install", r.id)
        || i.links.is_empty()
        || i.links.len() > 2
        || i.request.agents.len() != i.links.len()
        || !["user", "project"].contains(&i.request.scope.as_str())
        || !install_source::segment(&i.manifest.name)
        || i.entries.iter().any(|e| {
            !install_source::relative(&e.path)
                || e.path == "."
                || e.path
                    .split('/')
                    .any(|p| [".git", ".skillrail"].contains(&p.to_ascii_lowercase().as_str()))
        })
        || i.entries.windows(2).any(|w| w[0].path >= w[1].path)
        || i.manifest.files != i.entries.iter().map(|e| e.path.clone()).collect::<Vec<_>>()
    {
        return Err(journal_error());
    }
    for (n, l) in i.links.iter().enumerate() {
        let a = &i.request.agents[n];
        let base = if i.request.scope == "user" {
            &r.context.home
        } else {
            &r.context.project
        };
        let root = base.join(match a.as_str() {
            "claude" => ".claude/skills",
            "codex" => ".agents/skills",
            _ => return Err(journal_error()),
        });
        if l.root.path != root
            || l.path != root.join(&i.manifest.name)
            || !l.root.path.starts_with(&l.root.anchor)
            || l.published
                .as_ref()
                .is_some_and(|s| s.bytes != i.manifest.snapshot.to_string_lossy().as_bytes())
        {
            return Err(journal_error());
        }
    }
    if i.manifest.links != i.links.iter().map(|l| l.path.clone()).collect::<Vec<_>>()
        || i.links
            .iter()
            .map(|l| &l.path)
            .collect::<BTreeSet<_>>()
            .len()
            != i.links.len()
        || !i.store.path.starts_with(&i.store.anchor)
        || i.files.keys().any(|p| {
            p != ".skillrail/manifest.json"
                && p != ".skillrail"
                && !i.entries.iter().any(|e| &e.path == p)
        })
    {
        return Err(journal_error());
    }
    Ok(())
}
fn absent(path: &Path) -> bool {
    fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
}
fn manifest_bytes(i: &Install) -> Result<Vec<u8>, AppError> {
    serde_json::to_vec(&i.manifest).map_err(|_| journal_error())
}
fn expected(i: &Install) -> Result<Vec<Entry>, AppError> {
    let mut e = i.entries.clone();
    e.push(Entry {
        path: ".skillrail/manifest.json".into(),
        bytes: manifest_bytes(i)?,
        executable: false,
        directory: false,
    });
    Ok(e)
}
fn directory_names(entries: &[Entry]) -> Vec<PathBuf> {
    let mut dirs = BTreeSet::new();
    for e in entries {
        if e.directory {
            dirs.insert(PathBuf::from(&e.path));
        }
        for p in Path::new(&e.path).ancestors().skip(1) {
            if !p.as_os_str().is_empty() {
                dirs.insert(p.to_owned());
            }
        }
    }
    let mut dirs: Vec<_> = dirs.into_iter().collect();
    dirs.sort_by_key(|p| p.components().count());
    dirs
}
fn tree_entries(root: &Path) -> Result<Vec<String>, AppError> {
    fn walk(root: &Path, path: &Path, out: &mut Vec<String>, depth: usize) -> Result<(), AppError> {
        if depth > 34 || out.len() > 2002 {
            return Err(conflict());
        }
        let d = Directory::open(path)?;
        for e in fs::read_dir(path).map_err(|_| conflict())?.take(2003) {
            let e = e.map_err(|_| conflict())?;
            let p = e.path();
            let rel = p
                .strip_prefix(root)
                .map_err(|_| conflict())?
                .to_str()
                .ok_or_else(conflict)?
                .to_owned();
            if fs::symlink_metadata(&p).map_err(|_| conflict())?.is_dir() {
                out.push(format!("{rel}/"));
                walk(root, &p, out, depth + 1)?;
            } else {
                d.read(filename(&p)?, false)?.ok_or_else(conflict)?;
                out.push(rel);
            }
        }
        Ok(())
    }
    let mut out = vec![];
    walk(root, root, &mut out, 0)?;
    Ok(out)
}
fn check_tree(i: &Install, path: &Path, partial: bool) -> Result<(), AppError> {
    let d = Directory::open(path)?;
    if let Some(chain) = &i.tree_chain {
        // Stage-to-snapshot rename changes only the lexical basename, not descriptor identities.
        if &d.chain != chain {
            return Err(conflict());
        }
    }
    let entries = expected(i)?;
    let dirs = directory_names(&entries);
    let actual = tree_entries(path)?;
    for name in &actual {
        if let Some(p) = name.strip_suffix('/') {
            if let Some(identity) = i.files.get(p) {
                if Directory::open(&path.join(p))?.chain.last() != Some(identity) {
                    return Err(conflict());
                }
            } else if !partial {
                return Err(conflict());
            }
            if !dirs.iter().any(|d| d == Path::new(p)) {
                return Err(conflict());
            }
            continue;
        }
        let e = entries
            .iter()
            .find(|e| &e.path == name)
            .ok_or_else(conflict)?;
        let p = path.join(name);
        let d = Directory::open(p.parent().ok_or_else(journal_error)?)?;
        let s = d.read(filename(&p)?, false)?.ok_or_else(conflict)?;
        if let Some(identity) = i.files.get(name) {
            if &s.identity != identity || s.bytes != e.bytes {
                return Err(conflict());
            }
        } else if !partial || !e.bytes.starts_with(&s.bytes) {
            return Err(conflict());
        }
        if !partial && (s.mode() & 0o111 != 0) != e.executable {
            return Err(conflict());
        }
    }
    if !partial
        && entries.iter().any(|e| {
            !actual.contains(&if e.directory {
                format!("{}/", e.path)
            } else {
                e.path.clone()
            })
        })
    {
        return Err(conflict());
    }
    Ok(())
}
fn link_dir(l: &Link) -> Result<Option<Directory>, AppError> {
    l.root.check()?;
    if absent(&l.root.path) {
        return Ok(None);
    }
    let d = Directory::open(&l.root.path)?;
    if l.parents.as_ref().is_some_and(|p| p != &d.chain) {
        return Err(conflict());
    }
    Ok(Some(d))
}
fn validate_consumers(app: &Application, i: &Install) -> Result<(), AppError> {
    let inventory = crate::discovery::discover(&app.paths, 0);
    if inventory.occurrences.iter().any(|o| {
        o.canonical_path
            .clone()
            .or_else(|| o.original_path.canonicalize().ok())
            .is_some_and(|p| p.starts_with(&i.manifest.snapshot))
            && !i.manifest.links.contains(&o.original_path)
    }) {
        return Err(refused(
            "An unowned discovered consumer references this snapshot; preserve it and resolve that consumer before undo.",
        ));
    }
    Ok(())
}
pub(super) fn validate_undo(app: &Application, r: &Record) -> Result<(), AppError> {
    let i = r.install.as_ref().ok_or_else(journal_error)?;
    if i.journal != app.paths.state_dir {
        return Err(refused(
            "Install belongs to another private journal; use its original --state-dir.",
        ));
    }
    if r.status == "undone" {
        return Ok(());
    }
    i.store.check()?;
    validate_consumers(app, i)?;
    for l in &i.links {
        if let Some(d) = link_dir(l)? {
            let actual = d.link(filename(&l.path)?)?;
            if actual.is_some() && actual != l.published {
                return Err(conflict());
            }
            // An absent link is acceptable during interrupted apply/undo, never another target.
        }
        let stage = l.path.parent().ok_or_else(journal_error)?.join(&i.stage);
        if !absent(&stage) {
            let d = Directory::open(&stage)?;
            let actual = d.link("link")?;
            if let Some(actual) = &actual
                && (l.published.as_ref().is_some_and(|p| p != actual)
                    || actual.bytes != i.manifest.snapshot.to_string_lossy().as_bytes())
            {
                return Err(conflict());
            }
            if fs::read_dir(&stage).map_err(|_| conflict())?.count() > usize::from(actual.is_some())
            {
                return Err(conflict());
            }
        }
    }
    let stage = i.store.path.join(&i.stage);
    if !absent(&i.manifest.snapshot) {
        check_tree(i, &i.manifest.snapshot, r.status == "undoing")?;
    }
    if !absent(&stage) {
        check_tree(i, &stage, true)?;
    }
    Ok(())
}
fn check_names(app: &Application, i: &Install) -> Result<(), AppError> {
    let inventory = crate::discovery::discover(&app.paths, 0);
    if inventory.occurrences.iter().any(|o| {
        i.request.agents.iter().any(|a| agent(a) == o.agent)
            && o.canonical_path.as_ref() != Some(&i.manifest.snapshot)
            && (o.name.eq_ignore_ascii_case(&i.manifest.name)
                || o.invocations
                    .iter()
                    .any(|v| v.name.eq_ignore_ascii_case(&i.manifest.name)))
    }) {
        return Err(refused(
            "A selected agent already has this native name or invocation alias in the project context; resolve that collision without changing its owner.",
        ));
    }
    Ok(())
}
fn check_destinations(i: &Install) -> Result<(), AppError> {
    i.store.check()?;
    if !absent(&i.manifest.snapshot) || !absent(&i.store.path.join(&i.stage)) {
        return Err(conflict());
    }
    for l in &i.links {
        l.root.check()?;
        if link_dir(l)?.is_some() {
            // Refuse case aliases and any existing content, including dangling links.
            if !absent(&l.path) {
                return Err(conflict());
            }
            for e in fs::read_dir(&l.root.path)
                .map_err(|_| conflict())?
                .take(10001)
            {
                let e = e.map_err(|_| conflict())?;
                if e.file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&i.manifest.name)
                {
                    return Err(conflict());
                }
            }
        }
    }
    Ok(())
}
impl Application {
    pub fn plan_install(&self, mut request: InstallRequest) -> Result<Plan, AppError> {
        request.agents.sort();
        let source = install_source::collect(&request)?;
        let owned_id = crate::files::id(&[
            "owned",
            &serde_json::to_string(&source.provenance).map_err(|_| journal_error())?,
        ]);
        let store = Root::preview(self.paths.home.join(".skillrail-sources"))?;
        let snapshot = store.path.join(&owned_id);
        let mut links = vec![];
        for agent in &request.agents {
            let base = if request.scope == "user" {
                &self.paths.home
            } else {
                &self.paths.project
            };
            let root = Root::preview(base.join(if agent == "claude" {
                ".claude/skills"
            } else {
                ".agents/skills"
            }))?;
            links.push(Link {
                path: root.path.join(&source.name),
                root,
                parents: None,
                published: None,
            });
        }
        if links.iter().map(|l| &l.path).collect::<BTreeSet<_>>().len() != links.len() {
            return Err(conflict());
        }
        let manifest = Manifest {
            version: 1,
            owned_id,
            name: source.name,
            provenance: source.provenance,
            snapshot,
            links: links.iter().map(|l| l.path.clone()).collect(),
            files: source.entries.iter().map(|e| e.path.clone()).collect(),
        };
        let id = random_id()?;
        let install = Install {
            request,
            journal: self.paths.state_dir.clone(),
            manifest,
            entries: source.entries,
            store,
            stage: format!(".skillrail-{id}-install"),
            tree_chain: None,
            files: BTreeMap::new(),
            links,
        };
        check_names(self, &install)?;
        check_destinations(&install)?;
        let record = Record {
            version: 3,
            id,
            status: "prepared".into(),
            context: (&self.paths).into(),
            requests: vec![],
            changes: vec![],
            undo_changes: vec![],
            install: Some(install),
        };
        admit(&record)?;
        plan(record, false, None)
    }
}
pub(super) fn plan(record: Record, undo: bool, before: Option<Snapshot>) -> Result<Plan, AppError> {
    let i = record.install.as_ref().ok_or_else(journal_error)?;
    let encoded = serde_json::to_string(&(
        &record.context,
        &i.request,
        &i.journal,
        &i.manifest,
        &i.store,
        &i.links
            .iter()
            .map(|l| (&l.path, &l.root))
            .collect::<Vec<_>>(),
        undo,
        &before,
    ))
    .map_err(|_| journal_error())?;
    let mut changes = vec![FileChangeView {
        path: i.manifest.snapshot.clone(),
        action: if undo {
            "remove_owned_snapshot"
        } else {
            "create_owned_snapshot"
        },
    }];
    changes.extend(i.links.iter().map(|l| FileChangeView {
        path: l.path.clone(),
        action: if undo {
            "remove_owned_link"
        } else {
            "create_selected_link"
        },
    }));
    let view = PlanView {
        schema_version: 1,
        status: "preview".into(),
        action: if undo { "undo" } else { "install" }.into(),
        operation_id: record.id.clone(),
        token: crate::files::id(&[&encoded]),
        files: changes.iter().map(|c| c.path.clone()).collect(),
        changes,
        targets: vec![],
        notes: vec![
            "Snapshot content is copied; source scripts are never executed. Links select configured locations; session loading remains unresolved.",
            "Apply revalidates source and destinations. Undo verifies owned content and discovered consumers. Created empty parent directories are retained.",
        ],
        install: Some(i.manifest.clone()),
    };
    Ok(Plan {
        record,
        view,
        undo,
        record_before: before,
        inventory_revision: None,
        preconditions: vec![],
    })
}
pub(super) fn admit(r: &Record) -> Result<(), AppError> {
    let mut worst = r.clone();
    let i = worst.install.as_mut().ok_or_else(journal_error)?;
    let max = Snapshot::reservation(&[]).identity;
    // Bound all future inode metadata and directory chains; bytes appear only in entries.
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
    if serde_json::to_vec(&worst)
        .map_err(|_| journal_error())?
        .len()
        > journal_record::LIMIT
    {
        return Err(refused(
            "Complete install recovery exceeds the 4 MiB private record budget; select a smaller skill.",
        ));
    }
    Ok(())
}
/// Manifest ownership is read offline and only from the fixed user store.
pub(crate) fn owned_source(paths: &Paths, path: &Path) -> Option<crate::provenance::Source> {
    let store = paths.home.join(".skillrail-sources");
    if path.parent() != Some(store.as_path()) {
        return None;
    }
    let d = Directory::open(&path.join(".skillrail")).ok()?;
    let s = d.read("manifest.json", true).ok()??;
    let m: Manifest = serde_json::from_slice(&s.bytes).ok()?;
    if m.version != 1
        || m.snapshot != path
        || path.file_name()?.to_str()? != m.owned_id
        || !m.provenance.valid()
        || m.owned_id != crate::files::id(&["owned", &serde_json::to_string(&m.provenance).ok()?])
        || m.owned_id.len() != 64
    {
        return None;
    }
    Some(crate::provenance::Source {
        identity: crate::files::id(&["owned-manifest", &m.owned_id]),
        evidence: "owned_snapshot_manifest",
        installed: Some(m.provenance.clone()),
        repository: Some(m.provenance.origin),
        subpath: Some(m.provenance.subpath),
        owner: "skillrail",
    })
}
