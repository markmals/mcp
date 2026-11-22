//! One client connection: one request line in, one reply line out.

use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::OwnedReadHalf;

use super::dispatch::{Daemon, daemon_error};
use super::protocol::{Reply, Request};
use crate::failure::Failure;

/// Serves one exchange. A client that closes the connection before the reply cancels the
/// request: the request's future is dropped, which cancels anything it sent a server.
pub(crate) async fn serve(daemon: &Daemon, stream: UnixStream) {
    let _busy = daemon.busy();
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut line = Vec::new();
    match reader.read_until(b'\n', &mut line).await {
        Ok(0) | Err(_) => return,
        Ok(_) => {}
    }
    let reply = match serde_json::from_slice::<Request>(&line) {
        Ok(request) => tokio::select! {
            reply = daemon.handle(request) => reply,
            () = closed(&mut reader) => return,
        },
        Err(error) => Reply::Error(daemon_error(Failure::internal(format!(
            "the session daemon could not read the request: {error}"
        )))),
    };
    let mut bytes = match serde_json::to_vec(&reply) {
        Ok(bytes) => bytes,
        Err(error) => unwritable(&error),
    };
    bytes.push(b'\n');
    // A client that has gone away needs no reply.
    let _ = write.write_all(&bytes).await;
    let _ = write.shutdown().await;
}

/// Resolves when the client closes its end; the client writes nothing after its request.
async fn closed(reader: &mut BufReader<OwnedReadHalf>) {
    let mut ignored = [0_u8; 64];
    while let Ok(1..) = reader.read(&mut ignored).await {}
}

fn unwritable(error: &serde_json::Error) -> Vec<u8> {
    let failure =
        Failure::internal(format!("the session daemon could not write its reply: {error}"));
    let error = daemon_error(failure);
    json!({ "error": { "code": error.code, "message": error.message, "details": {}, "suggestions": [] } })
        .to_string()
        .into_bytes()
}
