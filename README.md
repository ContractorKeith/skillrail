# skillrail

A local-first skill and MCP registry for Claude Code and Codex. Understand
where skills come from, which configurations expose them, and where local
copies differ. Deliberate changes come with previews and conflict-aware undo.

**Status:** planning scaffold; not yet installable. Rust, Ratatui, SQLite;
Linux and macOS. See [the build plan](docs/PLAN.md) for scope and acceptance.

## Development

The M0 milestone establishes the Rust crate and CI. The shared acceptance
entrypoint is `bash scripts/accept.sh`; it intentionally fails until M0 exists.

## License

MIT — see [LICENSE](LICENSE).
