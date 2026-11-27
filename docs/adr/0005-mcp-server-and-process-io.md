# 0005: MCP server concurrency and process I/O

Status: accepted (recorded from code comments, 2026-11-27)

## Decisions
### Read-only stdio MCP (`src/mcp_server.rs`, `src/mcp_transport.rs`)
- No raw resources, skill bodies or mutation capability.
- Scans run on a single bounded blocking worker. Scans are never queued, and
  the protocol runtime is never blocked. A busy scan gets a busy response, and
  scan workers never pile up.
- The runtime gets `max_blocking_threads(4)`, because Tokio's stdin and stdout
  each need blocking capacity alongside the scan.
- EOF is observed before rmcp's five-second response drain. A disconnected
  client can't consume those responses, so it must not wait for an in-flight
  scan. Shutdown waits at most 100 ms for a slow filesystem worker.
- The two SDK transports share one frame writer, and its lock is held from the
  first write through flush, so normal replies and parser errors can't
  interleave frames.
- Receive work is preserved across the service loop's `select!` cancellation.
  The receive future is cancelled only on close or drop. Close awaits the
  reader task so a parser-error frame lock is released first.
- Pinned SDK behavior the tests rely on: the transport discards syntax errors
  and oversized lines, then resumes. Malformed outer params are rejected
  before tool validation.
- Tests use the official rmcp SDK client against the built executable, not an
  in-process handler.

### CLI output (`src/output.rs`) and terminal (`src/tui.rs`)
- stdout is flushed explicitly, so buffered write failures follow the same
  error contract as any other failure.
- If writing an error to stderr fails too, the error exit code is kept and
  nothing panics.
- Terminal restoration runs in `Drop`, which also covers I/O errors and
  unwinding. Ratatui installs its own panic hook.
