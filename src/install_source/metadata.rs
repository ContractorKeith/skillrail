//! Admission checks for selected native adapters, separate from inventory diagnostics.
use super::*;

pub(super) fn validate(text: &str, agents: &[String]) -> Result<String, AppError> {
    let metadata = crate::discovery::parse_metadata(text).map_err(|_| unsafe_source())?;
    let name = metadata
        .name
        .filter(|s| {
            segment(s)
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        })
        .ok_or_else(unsafe_source)?;
    let description = metadata.description.ok_or_else(unsafe_source)?;
    if description.split_whitespace().next().is_none() || description.len() > 4096 {
        return Err(unsafe_source());
    }
    for agent in agents {
        match agent.as_str() {
            "codex" => {
                // Pinned Codex 6ff1e1ca skills/src/parser.rs: 64 characters,
                // nonblank sanitized description, typed optional metadata.
                if name.chars().count() > 64 {
                    return Err(unsafe_source());
                }
                #[derive(Deserialize)]
                struct CodexFields {
                    #[serde(default)]
                    metadata: CodexMetadata,
                }
                #[derive(Default, Deserialize)]
                struct CodexMetadata {
                    #[serde(rename = "short-description")]
                    _short_description: Option<String>,
                }
                let front = text
                    .lines()
                    .skip(1)
                    .take_while(|line| *line != "---")
                    .collect::<Vec<_>>()
                    .join("\n");
                let parsed: CodexFields =
                    serde_saphyr::from_str(&front).map_err(|_| unsafe_source())?;
                let _ = parsed.metadata;
            }
            "claude" => {
                // The link's folder name equals frontmatter name. Claude reserves
                // synced case-insensitively, and the exact synced namespace.
                if name.eq_ignore_ascii_case("synced")
                    || name.eq_ignore_ascii_case("anthropic-skills")
                    || name.to_ascii_lowercase().starts_with("anthropic-skills:")
                {
                    return Err(unsafe_source());
                }
            }
            _ => return Err(unsafe_source()),
        }
    }
    Ok(name)
}
