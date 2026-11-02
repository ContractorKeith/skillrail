//! Preserve SDK receive work across the service loop's select cancellation.
use rmcp::{
    model::{ClientJsonRpcMessage, ServerJsonRpcMessage},
    service::RoleServer,
    transport::{Transport, async_rw::AsyncRwTransport},
};
use std::{
    future::Future,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, ready},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, Empty},
    sync::{Mutex, OwnedMutexGuard, mpsc},
    task::JoinHandle,
};

type WriterLock<W> = Pin<Box<dyn Future<Output = OwnedMutexGuard<W>> + Send>>;

// The two SDK transports share one writer. Keep its lock from the first write
// through flush, so normal replies and parser errors cannot interleave frames.
struct FrameWriter<W> {
    writer: Arc<Mutex<W>>,
    locking: Option<WriterLock<W>>,
    guard: Option<OwnedMutexGuard<W>>,
}
impl<W: Send + 'static> FrameWriter<W> {
    fn new(writer: Arc<Mutex<W>>) -> Self {
        Self {
            writer,
            locking: None,
            guard: None,
        }
    }
    fn poll_lock(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if self.guard.is_none() {
            let locking = self
                .locking
                .get_or_insert_with(|| Box::pin(self.writer.clone().lock_owned()));
            self.guard = Some(ready!(locking.as_mut().poll(cx)));
            self.locking = None;
        }
        Poll::Ready(())
    }
}
impl<W: AsyncWrite + Unpin + Send + 'static> AsyncWrite for FrameWriter<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        ready!(this.poll_lock(cx));
        let result = Pin::new(&mut **this.guard.as_mut().unwrap()).poll_write(cx, bytes);
        if matches!(&result, Poll::Ready(Err(_)))
            || (!bytes.is_empty() && matches!(&result, Poll::Ready(Ok(0))))
        {
            this.guard = None;
        }
        result
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        ready!(this.poll_lock(cx));
        let result = ready!(Pin::new(&mut **this.guard.as_mut().unwrap()).poll_flush(cx));
        this.guard = None;
        Poll::Ready(result)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_flush(cx)
    }
}

struct BoundedTransport<W: AsyncWrite + Unpin + Send + 'static> {
    outgoing: AsyncRwTransport<RoleServer, Empty, FrameWriter<W>>,
    incoming: mpsc::Receiver<ClientJsonRpcMessage>,
    reader_task: JoinHandle<()>,
}
impl<W: AsyncWrite + Unpin + Send + 'static> Transport<RoleServer> for BoundedTransport<W> {
    type Error = io::Error;
    fn send(
        &mut self,
        item: ServerJsonRpcMessage,
    ) -> impl Future<Output = io::Result<()>> + Send + 'static {
        self.outgoing.send(item)
    }
    async fn receive(&mut self) -> Option<ClientJsonRpcMessage> {
        self.incoming.recv().await
    }
    async fn close(&mut self) -> io::Result<()> {
        self.incoming.close();
        self.reader_task.abort();
        // Await cancellation to release any parser-error frame lock before close.
        let _ = (&mut self.reader_task).await;
        self.outgoing.close().await
    }
}
impl<W: AsyncWrite + Unpin + Send + 'static> Drop for BoundedTransport<W> {
    fn drop(&mut self) {
        self.reader_task.abort();
    }
}

pub(crate) fn bounded<R, W>(reader: R, writer: W) -> impl Transport<RoleServer, Error = io::Error>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let writer = Arc::new(Mutex::new(writer));
    let mut input = AsyncRwTransport::new_server(reader, FrameWriter::new(writer.clone()))
        .with_max_line_length(64 * 1024);
    let (incoming_tx, incoming) = mpsc::channel(1);
    let reader_task = tokio::spawn(async move {
        // This future is only cancelled on close/drop, never by ordinary sends.
        while let Some(message) = input.receive().await {
            if incoming_tx.send(message).await.is_err() {
                break;
            }
        }
    });
    BoundedTransport {
        outgoing: AsyncRwTransport::new_server(tokio::io::empty(), FrameWriter::new(writer)),
        incoming,
        reader_task,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::{Future, poll_fn},
        task::Poll,
        time::Duration,
    };
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

    #[tokio::test]
    async fn malformed_response_survives_cancelled_receive_while_writer_busy() {
        // One-byte capacity guarantees send holds the shared output lock while
        // the SDK parser consumes the invalid shape and waits to send its error.
        let (writer, mut output) = tokio::io::duplex(1);
        let input: &'static [u8] = b"{\"jsonrpc\":\"2.0\",\"id\":{},\"method\":\"ping\",\"params\":{\"secret\":\"DO_NOT_ECHO\"}}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n";
        let mut transport = bounded(input, writer);
        let response: rmcp::model::ServerJsonRpcMessage =
            serde_json::from_value(serde_json::json!({"jsonrpc":"2.0","id":1,"result":{}}))
                .unwrap();
        let response_len = serde_json::to_vec(&response).unwrap().len() + 1;
        let mut sending = Box::pin(transport.send(response));
        poll_fn(|cx| {
            assert!(sending.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        let mut receiving = Box::pin(transport.receive());
        poll_fn(|cx| {
            assert!(receiving.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        // This is the rmcp service select losing to an outgoing response event.
        drop(receiving);
        let mut first = vec![0; response_len];
        let (sent, read) = tokio::join!(sending, output.read_exact(&mut first));
        sent.unwrap();
        read.unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&first).unwrap()["id"],
            1
        );
        let mut output = BufReader::new(output);
        let read_error = async {
            let mut line = String::new();
            output.read_line(&mut line).await.unwrap();
            let error: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(error["error"]["code"], -32600);
            assert!(error["id"].is_null());
            assert!(!line.contains("DO_NOT_ECHO"));
        };
        tokio::time::timeout(Duration::from_secs(1), async {
            let (message, ()) = tokio::join!(transport.receive(), read_error);
            let message = serde_json::to_value(message.unwrap()).unwrap();
            assert_eq!(message["id"], 2);
        })
        .await
        .expect("cancelled receive lost the malformed-request response");
    }
    #[tokio::test]
    async fn cancelled_receive_during_error_flush_preserves_complete_frames() {
        let (writer, mut output) = tokio::io::duplex(1);
        let input: &'static [u8] = b"{\"jsonrpc\":\"2.0\",\"id\":{},\"method\":\"ping\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n";
        let mut transport = bounded(input, writer);
        let mut receiving = Box::pin(transport.receive());
        poll_fn(|cx| {
            assert!(receiving.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        let mut prefix = [0];
        tokio::time::timeout(Duration::from_secs(1), output.read_exact(&mut prefix))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(prefix[0], b'{'); // Error flush has actually reached backpressure.
        drop(receiving);
        let response =
            serde_json::from_value(serde_json::json!({"jsonrpc":"2.0","id":3,"result":{}}))
                .unwrap();
        let send = transport.send(response);
        let read_frames = async {
            let mut output = BufReader::new(output);
            let mut error = String::from("{");
            output.read_line(&mut error).await.unwrap();
            let error: serde_json::Value = serde_json::from_str(&error).unwrap();
            assert_eq!(error["error"]["code"], -32600);
            let mut reply = String::new();
            output.read_line(&mut reply).await.unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&reply).unwrap()["id"],
                3
            );
        };
        tokio::time::timeout(Duration::from_secs(1), async {
            let (sent, received, ()) = tokio::join!(send, transport.receive(), read_frames);
            sent.unwrap();
            assert_eq!(serde_json::to_value(received.unwrap()).unwrap()["id"], 2);
        })
        .await
        .expect("cancelled receive corrupted or lost a complete frame");
        transport.close().await.unwrap();
    }

    #[tokio::test]
    async fn partial_line_survives_receive_cancellation_and_close_releases_reader() {
        use tokio::io::AsyncWriteExt;
        let (mut client, input) = tokio::io::duplex(128);
        let mut transport = bounded(input, tokio::io::sink());
        client
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":7,")
            .await
            .unwrap();
        let mut receiving = Box::pin(transport.receive());
        poll_fn(|cx| {
            assert!(receiving.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        drop(receiving);
        client.write_all(b"\"method\":\"ping\"}\n").await.unwrap();
        let message = tokio::time::timeout(Duration::from_secs(1), transport.receive())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(serde_json::to_value(message).unwrap()["id"], 7);
        transport.close().await.unwrap();
        assert!(client.write_all(b"after close").await.is_err());
        assert!(transport.receive().await.is_none());
    }

    #[tokio::test]
    async fn close_aborts_backpressured_error_and_drop_stops_bounded_reader() {
        let (writer, mut output) = tokio::io::duplex(1);
        let input: &'static [u8] = b"{\"jsonrpc\":\"2.0\",\"id\":{},\"method\":\"ping\"}\n";
        let mut transport = bounded(input, writer);
        tokio::time::timeout(Duration::from_secs(1), output.read_exact(&mut [0]))
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), transport.close())
            .await
            .unwrap()
            .unwrap();
        assert!(transport.receive().await.is_none());
        let mut tail = Vec::new();
        tokio::time::timeout(Duration::from_secs(1), output.read_to_end(&mut tail))
            .await
            .unwrap()
            .unwrap();

        let (mut client, input) = tokio::io::duplex(128);
        let transport = bounded(input, tokio::io::sink());
        // Fill the bounded request channel and leave the one reader backpressured.
        use tokio::io::AsyncWriteExt;
        client.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n").await.unwrap();
        drop(transport);
        let mut bytes = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), client.read(&mut bytes))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    }
    struct FailedWriter;
    impl AsyncWrite for FailedWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
    #[tokio::test]
    async fn failed_outgoing_write_does_not_strand_parser_error_writer() {
        use tokio::io::AsyncWriteExt;
        let (mut client, input) = tokio::io::duplex(128);
        let mut transport = bounded(input, FailedWriter);
        let response =
            serde_json::from_value(serde_json::json!({"jsonrpc":"2.0","id":1,"result":{}}))
                .unwrap();
        assert!(transport.send(response).await.is_err());
        client
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":{},\"method\":\"ping\"}\n")
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), transport.receive())
                .await
                .expect("failed send retained the shared frame lock")
                .is_none()
        );
        transport.close().await.unwrap();
    }
}
