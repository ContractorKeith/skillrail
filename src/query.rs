//! One query seam over sanitized discovery facts, including TUI snapshots.
use crate::{AppError, Application, Inventory, discovery::Agent};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
pub const DEFAULT_LIMIT: usize = 100;
pub const MAX_LIMIT: usize = 200;

#[derive(Debug, Clone, Deserialize, clap::Args)]
#[serde(default, deny_unknown_fields)]
pub struct Query {
    /// Item kind: skill or mcp.
    #[arg(long)]
    pub kind: Option<String>,
    /// Agent occurrence: claude or codex.
    #[arg(long)]
    pub agent: Option<String>,
    /// Occurrence scope: user, project, local or unknown.
    #[arg(long)]
    pub scope: Option<String>,
    /// Maximum items (1..200, default 100).
    #[arg(long, default_value_t = DEFAULT_LIMIT)]
    pub limit: usize,
    /// Item offset; requires --revision when nonzero.
    #[arg(long, default_value_t = 0)]
    pub offset: usize,
    /// Inventory revision returned by the previous page.
    #[arg(long)]
    pub revision: Option<String>,
    #[arg(skip)]
    pub query: Option<String>,
    #[arg(skip)]
    pub id: Option<String>,
    #[arg(skip)]
    pub drift_only: bool,
}
impl Default for Query {
    fn default() -> Self {
        Self {
            kind: None,
            agent: None,
            scope: None,
            limit: DEFAULT_LIMIT,
            offset: 0,
            revision: None,
            query: None,
            id: None,
            drift_only: false,
        }
    }
}
fn invalid() -> AppError {
    AppError::new(
        "invalid_arguments",
        "Invalid query: check filters, ID, query length, limit, offset and revision.",
    )
}
impl Query {
    pub fn validate(&self) -> Result<(), AppError> {
        if self
            .kind
            .as_deref()
            .is_some_and(|v| !["skill", "mcp"].contains(&v))
            || self
                .agent
                .as_deref()
                .is_some_and(|v| !["claude", "codex"].contains(&v))
            || self
                .scope
                .as_deref()
                .is_some_and(|v| !["user", "project", "local", "unknown"].contains(&v))
            || !(1..=MAX_LIMIT).contains(&self.limit)
            || self.offset > 1_000_000
            || (self.offset > 0 && self.revision.is_none())
            || self.revision.as_ref().is_some_and(|v| {
                v.len() != 64
                    || !v
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            })
            || self
                .query
                .as_ref()
                .is_some_and(|v| v.trim().is_empty() || v.len() > 256)
            || self
                .id
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 256)
        {
            return Err(invalid());
        }
        Ok(())
    }

    /// Indices into one snapshot; matching occurrences constrain each other.
    pub fn indices(&self, inventory: &Inventory) -> Vec<usize> {
        let needle = self.query.as_ref().map(|v| v.to_lowercase());
        let matching_occurrences: BTreeSet<_> = inventory
            .occurrences
            .iter()
            .filter(|o| self.matches_occurrence(o))
            .map(|o| &o.item_id)
            .collect();
        let mut indices: Vec<_> = inventory
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                self.kind.as_deref().is_none_or(|v| item.kind == v)
                    && self.id.as_ref().is_none_or(|v| item.id == *v)
                    && (!self.drift_only
                        || (item.kind == "skill" && ["drifted", "unknown"].contains(&item.drift)))
                    && needle.as_ref().is_none_or(|v| {
                        item.name.to_lowercase().contains(v)
                            || item
                                .description
                                .as_deref()
                                .unwrap_or("")
                                .to_lowercase()
                                .contains(v)
                            || item.id.contains(v)
                    })
                    && ((self.agent.is_none() && self.scope.is_none())
                        || matching_occurrences.contains(&item.id))
            })
            .map(|(index, _)| index)
            .collect();
        indices.sort_by(|a, b| inventory.items[*a].id.cmp(&inventory.items[*b].id));
        indices
    }
    fn matches_occurrence(&self, o: &crate::discovery::Occurrence) -> bool {
        self.agent.as_deref().is_none_or(|v| {
            matches!(
                (v, o.agent),
                ("claude", Agent::Claude) | ("codex", Agent::Codex)
            )
        }) && self.scope.as_deref().is_none_or(|v| {
            v == match o.scope {
                crate::discovery::Scope::User => "user",
                crate::discovery::Scope::Project => "project",
                crate::discovery::Scope::Local => "local",
                crate::discovery::Scope::Unknown => "unknown",
            }
        })
    }
}

#[derive(Debug, Serialize)]
pub struct Page {
    pub revision: String,
    pub offset: usize,
    pub limit: usize,
    pub total: usize,
    pub next_offset: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct QueryResult {
    #[serde(flatten)]
    pub inventory: Inventory,
    pub page: Page,
}
impl Application {
    pub fn query(&self, query: &Query) -> Result<QueryResult, AppError> {
        query.validate()?;
        query_snapshot(self.scan()?, query)
    }
}

pub fn query_snapshot(mut inventory: Inventory, query: &Query) -> Result<QueryResult, AppError> {
    query.validate()?;
    // Timestamp is deliberately excluded; any changed sanitized fact invalidates pages.
    let mut hash = HashWriter(Sha256::new());
    serde_json::to_writer(
        &mut hash,
        &(
            &inventory.context,
            &inventory.discovery,
            &inventory.items,
            &inventory.occurrences,
            &inventory.diagnostics,
        ),
    )
    .map_err(|_| invalid())?;
    let revision = hash
        .0
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if query.revision.as_ref().is_some_and(|v| *v != revision) {
        return Err(AppError::new(
            "inventory_changed",
            "Inventory changed; restart pagination without offset or revision.",
        ));
    }
    let indices = query.indices(&inventory);
    if query.id.is_some() && indices.is_empty() {
        return Err(AppError::new(
            "item_not_found",
            "No item with this ID exists in the selected inventory.",
        ));
    }
    let total = indices.len();
    let ids: BTreeSet<_> = indices
        .into_iter()
        .skip(query.offset)
        .take(query.limit)
        .map(|i| inventory.items[i].id.clone())
        .collect();
    inventory.items.retain(|i| ids.contains(&i.id));
    inventory.items.sort_by(|a, b| a.id.cmp(&b.id));
    // Keep all occurrences of matching items so visibility/provenance explanations are complete.
    inventory.occurrences.retain(|o| ids.contains(&o.item_id));
    inventory.occurrences.sort_by(|a, b| a.id.cmp(&b.id));
    let result = QueryResult {
        inventory,
        page: Page {
            revision,
            offset: query.offset,
            limit: query.limit,
            total,
            next_offset: (query.offset.saturating_add(query.limit) < total)
                .then_some(query.offset + query.limit),
        },
    };
    encode(&result)?;
    Ok(result)
}

struct HashWriter(Sha256);
impl std::io::Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
struct BoundedOutput(Vec<u8>);
impl std::io::Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_RESPONSE_BYTES {
            return Err(std::io::Error::other("response limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub fn encode(value: &impl Serialize) -> Result<String, AppError> {
    let mut output = BoundedOutput(Vec::new());
    serde_json::to_writer(&mut output, value).map_err(|_| {
        AppError::new(
            "response_too_large",
            "Result exceeds 1 MiB or cannot be encoded; narrow filters or reduce limit.",
        )
    })?;
    String::from_utf8(output.0)
        .map_err(|_| AppError::new("output_unavailable", "Cannot encode query result."))
}
