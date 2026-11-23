//! What the daemon does with each request, and how it decides it has nothing left to do.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use super::failures::timed_out;
use super::protocol::{DaemonError, DaemonStatus, Reply, ReplyBody, Request, SessionTarget};
use super::sessions::Sessions;
use crate::failure::Failure;

#[derive(Default)]
pub(crate) struct Daemon {
    pub(crate) sessions: Sessions,
    activity: Arc<Mutex<Activity>>,
}

struct Activity {
    /// Connections the daemon is serving now.
    in_flight: usize,
    last_active: Instant,
}

impl Default for Activity {
    fn default() -> Activity {
        Activity { in_flight: 0, last_active: Instant::now() }
    }
}

/// A connection being served; the daemon is busy until it drops.
pub(crate) struct Busy(Arc<Mutex<Activity>>);

impl Drop for Busy {
    fn drop(&mut self) {
        let mut activity = lock(&self.0);
        activity.in_flight -= 1;
        activity.last_active = Instant::now();
    }
}

impl Daemon {
    pub(crate) fn busy(&self) -> Busy {
        let mut activity = lock(&self.activity);
        activity.in_flight += 1;
        activity.last_active = Instant::now();
        Busy(Arc::clone(&self.activity))
    }

    /// How long the daemon has held no session and served no connection.
    pub(crate) fn idle_for(&self) -> Duration {
        let mut activity = lock(&self.activity);
        if activity.in_flight > 0 || !self.sessions.is_empty() {
            activity.last_active = Instant::now();
            return Duration::ZERO;
        }
        activity.last_active.elapsed()
    }

    pub(crate) async fn handle(&self, request: Request) -> Reply {
        let result = match request {
            Request::Status => Ok(ReplyBody::Status(DaemonStatus {
                pid: std::process::id(),
                sessions: self.sessions.summaries(),
            })),
            Request::Connect { target } => within_timeout(&target, self.connect(&target)).await,
            Request::ListTools { target } => {
                within_timeout(&target, self.list_tools(&target)).await
            }
            Request::CallTool { target, tool, arguments } => {
                within_timeout(&target, self.call_tool(&target, tool, arguments)).await
            }
            Request::Disconnect { workspace, servers } => Ok(ReplyBody::Disconnected {
                servers: self.sessions.disconnect(&workspace, servers.as_deref()).await,
            }),
        };
        match result {
            Ok(body) => Reply::Ok(body),
            Err(failure) => Reply::Error(daemon_error(failure)),
        }
    }

    async fn connect(&self, target: &SessionTarget) -> Result<ReplyBody, Failure> {
        let (session, reused) = self.sessions.acquire(target).await?;
        Ok(ReplyBody::Connected { session: session.info(reused), server: session.declaration()? })
    }

    async fn list_tools(&self, target: &SessionTarget) -> Result<ReplyBody, Failure> {
        let (session, reused) = self.sessions.acquire(target).await?;
        let tools = session.list_tools().await?;
        Ok(ReplyBody::Tools { session: session.info(reused), tools })
    }

    async fn call_tool(
        &self,
        target: &SessionTarget,
        tool: String,
        arguments: Map<String, Value>,
    ) -> Result<ReplyBody, Failure> {
        let (session, reused) = self.sessions.acquire(target).await?;
        let result = session.call_tool(tool, arguments).await?;
        Ok(ReplyBody::Called { session: session.info(reused), result })
    }
}

pub(crate) fn daemon_error(failure: Failure) -> DaemonError {
    DaemonError {
        code: failure.code,
        message: failure.message,
        details: *failure.details,
        suggestions: failure.suggestions,
    }
}

/// Bounds starting the session and the request together by the target's timeout. Dropping
/// `work` on expiry cancels any request it sent the server.
async fn within_timeout<T>(
    target: &SessionTarget,
    work: impl Future<Output = Result<T, Failure>>,
) -> Result<T, Failure> {
    let limit = Duration::from_millis(target.timeout_ms);
    tokio::time::timeout(limit, work)
        .await
        .unwrap_or_else(|_| Err(timed_out(&target.server, limit)))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
