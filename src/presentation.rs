//! Shared human-readable explanations. Escape every untrusted value before terminal output.
use crate::Inventory;

pub fn safe(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
                c.escape_default().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

pub fn details(inventory: &Inventory, selected: usize) -> String {
    let Some(item) = inventory.items.get(selected) else {
        return "No standalone skills found in supported locations.\nCoverage is partial; see diagnostics.\n".into();
    };
    let mut text = format!(
        "{} [{}]\nID: {}\nDrift: {} | Name collision: {}\nOwner: {}\nSource: {}\nEvidence: {}\n",
        safe(&item.name),
        item.kind,
        item.id,
        item.drift,
        item.name_collision,
        item.source.owner,
        item.source.identity,
        item.source.evidence
    );
    if let Some(description) = &item.description {
        text.push_str(&format!("Description: {}\n", safe(description)));
    }
    if let Some(repository) = &item.source.repository {
        text.push_str(&format!(
            "Repository: {}\nSubpath: {}\n",
            safe(repository),
            safe(item.source.subpath.as_deref().unwrap_or("unknown"))
        ));
    }
    for occurrence in inventory
        .occurrences
        .iter()
        .filter(|o| o.item_id == item.id)
    {
        text.push_str(&format!(
            "\n{:?}/{:?}: {}\nName: {}\nCanonical: {}\nVisibility: {}\n",
            occurrence.agent,
            occurrence.scope,
            safe(&occurrence.original_path.to_string_lossy()),
            safe(&occurrence.name),
            occurrence
                .canonical_path
                .as_ref()
                .map(|p| safe(&p.to_string_lossy()))
                .unwrap_or_else(|| "unknown".into()),
            occurrence.visibility.status
        ));
        for invocation in &occurrence.invocations {
            text.push_str(&format!(
                "Invocation: {} | shadowed in default layout: {}\n",
                safe(&invocation.name),
                invocation.shadowed_in_default_layout
            ));
        }
        for reason in &occurrence.visibility.reasons {
            text.push_str(&format!("  {}\n", safe(reason)));
        }
        text.push_str(&format!(
            "Fingerprint: {}\n",
            occurrence
                .fingerprint
                .sha256
                .as_deref()
                .unwrap_or("unknown")
        ));
        for problem in occurrence
            .diagnostics
            .iter()
            .chain(&occurrence.fingerprint.diagnostics)
        {
            text.push_str(&format!("  {}\n", safe(problem)));
        }
    }
    text
}
