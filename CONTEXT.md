# Skillrail

Language for the local skill and MCP registry.

## Language

**Source identity**:
The provenance that distinguishes a skill from unrelated skills, even when they share a name. Copies and symlinks belong together only when evidence establishes their connection.
_Avoid_: Name as identity

**Configured visibility**:
Whether an agent can discover an item from its configuration in the selected project context. This does not prove that a running session loaded or used it; unresolved visibility is unknown.
_Avoid_: Loaded, active, used

**Project context**:
One selected project together with the global locations that apply to it. The default selection is the current directory.
_Avoid_: All-project inventory

**Drift**:
A difference in full skill-folder content between related local copies, including scripts and references. A Git revision describes provenance; drift does not mean an upstream update is available.
_Avoid_: Outdated, update available

**Name collision**:
Distinct or unresolved source identities sharing a skill name. A matching name alone does not establish that the skills are copies.
_Avoid_: Duplicate solely by name

**Content owner**:
The tool or repository responsible for installing and updating an item's contents. Changing configured visibility does not transfer content ownership.
_Avoid_: Visibility manager as owner

**Skillrail-owned install**:
A skill installed into Skillrail's user-level source directory and linked into selected agent locations. Existing externally managed installations are not Skillrail-owned.
_Avoid_: Any discovered skill

**Undo record**:
A persistent record used to reverse a Skillrail change, provided intervening edits do not conflict with restoration.
_Avoid_: Unconditional rollback

**MCP configuration status**:
The presence, configured enablement, and validity of a server entry for an agent. This is not evidence that the server is running or reachable.
_Avoid_: Healthy, connected, running

**Standalone MCP entry**:
A server configuration managed independently of an agent plugin. Only supported standalone entries can be toggled by Skillrail in v1.
_Avoid_: Plugin-owned server

**Installed snapshot**:
A Skillrail-owned copy of a selected skill with recorded source provenance and content hash. Later source edits take effect only through an explicit update.
_Avoid_: Live source link

**Preview**:
The proposed changes shown before a mutation is applied. A preview itself changes no managed skill contents, agent configuration, or agent links.
_Avoid_: Applied change
