//! Read-only stdio MCP. No tracing subscriber: parser context cannot reach logs.
use crate::{
    AppError, Application,
    query::{self, Query},
};
use rmcp::{
    ErrorData, ServerHandler, ServiceExt,
    model::*,
    service::{RequestContext, RoleServer},
    transport::async_rw::AsyncRwTransport,
};
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, ReadBuf};

const TOOLS: [&str; 5] = [
    "list_inventory",
    "search_inventory",
    "get_item",
    "explain_visibility",
    "list_drift",
];
pub struct Server {
    application: Arc<Application>,
    scan_slot: Arc<tokio::sync::Semaphore>,
}
impl Server {
    pub fn new(application: Application) -> Self {
        Self {
            application: Arc::new(application),
            scan_slot: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }
}
fn invalid() -> ErrorData {
    ErrorData::invalid_params("Invalid tool arguments; consult tools/list schema.", None)
}
fn definition(name: &str) -> Tool {
    let details = ["get_item", "explain_visibility"].contains(&name);
    let mut properties = if details {
        json!({"id":{"type":"string","minLength":1,"maxLength":256}})
    } else {
        json!({
            "kind":{"type":"string","enum":["skill","mcp"]},
            "agent":{"type":"string","enum":["claude","codex"]},
            "scope":{"type":"string","enum":["user","project","local","unknown"]},
            "limit":{"type":"integer","minimum":1,"maximum":200,"default":100},
            "offset":{"type":"integer","minimum":0,"maximum":1000000,"default":0},
            "revision":{"type":"string","pattern":"^[a-f0-9]{64}$"}
        })
    };
    let required = if details {
        vec!["id"]
    } else if name == "search_inventory" {
        properties["query"] = json!({"type":"string","minLength":1,"maxLength":256});
        vec!["query"]
    } else {
        vec![]
    };
    Tool::new(name.to_owned(), match name {
        "list_inventory" => "Fresh sanitized inventory with stable-ID pagination.",
        "search_inventory" => "Search names, descriptions and IDs, case insensitive; fresh scan.",
        "get_item" => "Fresh item details and all occurrences; no skill body.",
        "explain_visibility" => "Fresh item and occurrence visibility reasons; configured facts do not prove session loading.",
        _ => "Fresh drifted skills and skills with unknown drift; no upstream lookup.",
    }, Arc::new(json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}).as_object().unwrap().clone()))
        .with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(true).open_world(false))
}
fn arguments(name: &str, args: Option<JsonObject>) -> Result<Query, ErrorData> {
    let args = args.unwrap_or_default();
    let definition = definition(name);
    let properties = definition.input_schema["properties"].as_object().unwrap();
    if args.keys().any(|k| !properties.contains_key(k)) || args.values().any(Value::is_null) {
        return Err(invalid());
    }
    let mut query: Query = serde_json::from_value(Value::Object(args)).map_err(|_| invalid())?;
    if (["get_item", "explain_visibility"].contains(&name) && query.id.is_none())
        || (name == "search_inventory" && query.query.is_none())
    {
        return Err(invalid());
    }
    query.drift_only = name == "list_drift";
    query.validate().map_err(|_| invalid())?;
    Ok(query)
}
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("skillrail", env!("CARGO_PKG_VERSION")))
    }
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Owned(vec![
            ProtocolVersion::V_2025_11_25,
            ProtocolVersion::V_2025_06_18,
            ProtocolVersion::V_2025_03_26,
            ProtocolVersion::V_2024_11_05,
        ])
    }
    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        if request.is_some_and(|r| r.cursor.is_some()) {
            return Err(invalid());
        }
        Ok(ListToolsResult {
            tools: TOOLS.iter().map(|name| definition(name)).collect(),
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if !TOOLS.contains(&request.name.as_ref()) {
            return Err(ErrorData::invalid_params("Unknown read tool.", None));
        }
        let query = arguments(&request.name, request.arguments)?;
        let permit = self.scan_slot.clone().try_acquire_owned().map_err(|_| {
            ErrorData::internal_error(
                "Inventory scan busy; retry after the current request.",
                None,
            )
        })?;
        let application = self.application.clone();
        // One bounded blocking worker; never queue scans or block the protocol runtime.
        let task = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            application.query(&query).and_then(|r| query::encode(&r))
        });
        let result = tokio::time::timeout(std::time::Duration::from_secs(30), task).await;
        let content = match result {
            Ok(Ok(Ok(text))) => CallToolResult::success(vec![ContentBlock::text(text)]),
            Ok(Ok(Err(error))) => CallToolResult::error(vec![ContentBlock::text(
                json!({"schema_version":1,"error":error}).to_string(),
            )]),
            _ => CallToolResult::error(vec![ContentBlock::text(
                "{\"schema_version\":1,\"error\":{\"code\":\"scan_unavailable\",\"message\":\"Scan failed or exceeded 30 seconds.\"}}",
            )]),
        };
        Ok(content.into())
    }
}

// Observe EOF before rmcp's five-second response drain: a disconnected client
// cannot consume those responses and must not wait for an in-flight scan.
struct DisconnectRead<R> {
    reader: R,
    closed: Option<tokio::sync::oneshot::Sender<()>>,
}
impl<R: AsyncRead + Unpin> AsyncRead for DisconnectRead<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let before = buffer.filled().len();
        let result = Pin::new(&mut this.reader).poll_read(cx, buffer);
        let disconnected = match &result {
            Poll::Ready(Err(_)) => true,
            Poll::Ready(Ok(())) => buffer.filled().len() == before,
            Poll::Pending => false,
        };
        if disconnected && let Some(closed) = this.closed.take() {
            let _ = closed.send(());
        }
        result
    }
}

pub fn run(application: Application) -> Result<(), AppError> {
    let unavailable = || {
        AppError::new(
            "mcp_unavailable",
            "MCP transport closed or initialization failed.",
        )
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        // Tokio stdin/stdout each need blocking capacity alongside the scan.
        .max_blocking_threads(4)
        .enable_all()
        .build()
        .map_err(|_| unavailable())?;
    let result = runtime.block_on(async {
        let (closed, disconnected) = tokio::sync::oneshot::channel();
        let input = DisconnectRead {
            reader: tokio::io::stdin(),
            closed: Some(closed),
        };
        let transport = AsyncRwTransport::new_server(input, tokio::io::stdout())
            .with_max_line_length(64 * 1024);
        let service = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            Server::new(application).serve(transport),
        )
        .await
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable())?;
        tokio::select! {
            result = service.waiting() => { result.map_err(|_| unavailable())?; }
            _ = disconnected => {}
        }
        Ok(())
    });
    // A disconnected client must never wait for a slow filesystem worker to finish.
    runtime.shutdown_timeout(std::time::Duration::from_millis(100));
    result
}
