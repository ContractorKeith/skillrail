//! Private v2 wire format stores each pre/post byte sequence once, including during undo.
use super::*;

pub(super) const LIMIT: usize = 4 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRecord {
    version: u32,
    id: String,
    status: String,
    context: Context,
    requests: Vec<ToggleRequest>,
    changes: Vec<StoredChange>,
    undo_changes: Vec<StoredUndo>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredChange {
    path: PathBuf,
    parents: Vec<Identity>,
    before: Option<Snapshot>,
    after: Vec<u8>,
    published: Option<Identity>,
    stage: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredUndo {
    change: usize,
    before: Option<Identity>,
    published: Option<Identity>,
    stage: String,
}
fn snapshot(identity: &Option<Identity>, bytes: &[u8]) -> Option<Snapshot> {
    identity.as_ref().map(|identity| Snapshot {
        identity: identity.clone(),
        bytes: bytes.to_vec(),
    })
}
pub(super) fn encode(record: &Record) -> Result<Vec<u8>, AppError> {
    if record.install.is_some() {
        return serde_json::to_vec(record).map_err(|_| journal_error());
    }
    let changes = record
        .changes
        .iter()
        .map(|c| {
            if c.published.as_ref().is_some_and(|p| p.bytes != c.after) {
                return Err(journal_error());
            }
            Ok(StoredChange {
                path: c.path.clone(),
                parents: c.parents.clone(),
                before: c.before.clone(),
                after: c.after.clone(),
                published: c.published.as_ref().map(|p| p.identity.clone()),
                stage: c.stage.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let undo_changes = record
        .undo_changes
        .iter()
        .map(|u| {
            let index = record
                .changes
                .iter()
                .position(|c| c.path == u.path)
                .ok_or_else(journal_error)?;
            let original = &record.changes[index];
            if u.parents != original.parents
                || u.after
                    != original
                        .before
                        .as_ref()
                        .map(|b| b.bytes.as_slice())
                        .unwrap_or_default()
                || u.before.as_ref().is_some_and(|p| p.bytes != original.after)
                || u.published.as_ref().is_some_and(|p| p.bytes != u.after)
            {
                return Err(journal_error());
            }
            Ok(StoredUndo {
                change: index,
                before: u.before.as_ref().map(|p| p.identity.clone()),
                published: u.published.as_ref().map(|p| p.identity.clone()),
                stage: u.stage.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let wire = StoredRecord {
        version: 2,
        id: record.id.clone(),
        status: record.status.clone(),
        context: record.context.clone(),
        requests: record.requests.clone(),
        changes,
        undo_changes,
    };
    serde_json::to_vec(&wire).map_err(|_| journal_error())
}
pub(super) fn decode(bytes: &[u8]) -> Result<Record, AppError> {
    #[derive(Deserialize)]
    struct Header {
        version: u32,
    }
    match serde_json::from_slice::<Header>(bytes)
        .map_err(|_| journal_error())?
        .version
    {
        1 | 3 => serde_json::from_slice(bytes).map_err(|_| journal_error()),
        2 => {
            let wire: StoredRecord = serde_json::from_slice(bytes).map_err(|_| journal_error())?;
            let changes: Vec<_> = wire
                .changes
                .into_iter()
                .map(|c| Change {
                    path: c.path,
                    parents: c.parents,
                    before: c.before,
                    published: snapshot(&c.published, &c.after),
                    after: c.after,
                    stage: c.stage,
                })
                .collect();
            let mut undo_changes: Vec<Change> = vec![];
            for u in wire.undo_changes {
                let c = changes.get(u.change).ok_or_else(journal_error)?;
                if undo_changes.iter().any(|prior| prior.path == c.path) {
                    return Err(journal_error());
                }
                let after = c
                    .before
                    .as_ref()
                    .map(|b| b.bytes.clone())
                    .unwrap_or_default();
                undo_changes.push(Change {
                    path: c.path.clone(),
                    parents: c.parents.clone(),
                    before: snapshot(&u.before, &c.after),
                    published: snapshot(&u.published, &after),
                    after,
                    stage: u.stage,
                });
            }
            Ok(Record {
                version: 2,
                id: wire.id,
                status: wire.status,
                context: wire.context,
                requests: wire.requests,
                changes,
                undo_changes,
                install: None,
            })
        }
        _ => Err(journal_error()),
    }
}

/// Reserve the largest possible serialized v2 state, before recording or publishing anything.
/// Bytes and parent chains never grow. Future file identities use maximum-width numbers;
/// all files can have both a published replacement and an undo stage. Undo is planned once.
pub(super) fn admit(record: &Record) -> Result<(), AppError> {
    if record.changes.is_empty() {
        return Ok(());
    }
    let content: usize = record
        .changes
        .iter()
        .map(|c| c.after.len() + c.before.as_ref().map_or(0, |s| s.bytes.len()))
        .sum();
    if content > LIMIT {
        return Err(capacity_error());
    }
    let mut worst = record.clone();
    worst.status = "prepared".into(); // longest supported status
    worst.undo_changes.clear();
    for c in &mut worst.changes {
        c.published = Some(Snapshot::reservation(&c.after));
        let after = c
            .before
            .as_ref()
            .map(|s| s.bytes.clone())
            .unwrap_or_default();
        worst.undo_changes.push(Change {
            path: c.path.clone(),
            parents: c.parents.clone(),
            before: Some(Snapshot::reservation(&c.after)),
            published: Some(Snapshot::reservation(&after)),
            after,
            stage: format!(".skillrail-{}-99-undo", record.id),
        });
    }
    if encode(&worst)?.len() > LIMIT {
        return Err(capacity_error());
    }
    Ok(())
}
fn capacity_error() -> AppError {
    refused(
        "Complete apply/undo recovery would exceed the 4 MiB private record budget; select fewer or smaller targets. No operation was admitted.",
    )
}
