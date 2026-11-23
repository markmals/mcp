//! Every session the daemon holds: at most one per workspace and server name.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use tokio::task::JoinSet;

use super::protocol::{SessionSummary, SessionTarget};
use super::session::{Session, SessionUse};
use crate::failure::Failure;

/// (workspace, server)
type Key = (String, String);

#[derive(Default)]
pub(crate) struct Sessions {
    slots: Mutex<HashMap<Key, Arc<Slot>>>,
    started: AtomicU64,
}

/// The place for one key's session. Slots are never removed, so two requests for one key always
/// share one slot.
#[derive(Default)]
struct Slot {
    /// Held while the slot's session is checked, started, replaced or ended, so concurrent
    /// requests for one key start one server. Shared so a sweep can hold it while a session
    /// stops in the background.
    changing: Arc<tokio::sync::Mutex<()>>,
    current: Mutex<Option<Arc<Session>>>,
}

impl Slot {
    fn current(&self) -> Option<Arc<Session>> {
        lock(&self.current).clone()
    }

    fn take(&self) -> Option<Arc<Session>> {
        lock(&self.current).take()
    }
}

impl Sessions {
    /// The target's session, and whether an earlier request started it. A session that ended or
    /// whose settings differ from the target's is replaced.
    pub(crate) async fn acquire(
        &self,
        target: &SessionTarget,
    ) -> Result<(SessionUse, bool), Failure> {
        let slot = self.slot(&target.workspace, &target.server);
        let _changing = slot.changing.lock().await;
        if let Some(session) = slot.current() {
            if session.is_alive() && session.transport.same_settings(&target.transport) {
                return Ok((session.begin_use(), true));
            }
            slot.take();
            session.stop().await;
        }
        let session = Arc::new(Session::start(target, self.next_id()).await?);
        let usage = session.begin_use();
        *lock(&slot.current) = Some(session);
        Ok((usage, false))
    }

    pub(crate) fn summaries(&self) -> Vec<SessionSummary> {
        let mut summaries: Vec<SessionSummary> =
            self.slots().iter().filter_map(|slot| slot.current()).map(|s| s.summary()).collect();
        summaries.sort_by(|a, b| (&a.workspace, &a.server).cmp(&(&b.workspace, &b.server)));
        summaries
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.slots().iter().all(|slot| lock(&slot.current).is_none())
    }

    /// Ends the sessions in `workspace` with a name in `servers`, or all of them when it is
    /// `None`; returns the names of the servers whose sessions ended.
    pub(crate) async fn disconnect(
        &self,
        workspace: &str,
        servers: Option<&[String]>,
    ) -> Vec<String> {
        let matching: Vec<(String, Arc<Slot>)> = lock(&self.slots)
            .iter()
            .filter(|((slot_workspace, server), _)| {
                slot_workspace == workspace && servers.is_none_or(|names| names.contains(server))
            })
            .map(|((_, server), slot)| (server.clone(), Arc::clone(slot)))
            .collect();
        let mut stopping = JoinSet::new();
        for (server, slot) in matching {
            // The slot stays locked until the old process is gone, so a request that arrives
            // meanwhile waits rather than starting a second server for the same key.
            stopping.spawn(async move {
                let _changing = slot.changing.lock().await;
                let session = slot.take()?;
                session.stop().await;
                Some(server)
            });
        }
        let mut ended: Vec<String> = stopping.join_all().await.into_iter().flatten().collect();
        ended.sort();
        ended
    }

    /// Ends sessions whose server is gone and sessions without a request for `idle_limit`.
    /// Slots a request is changing are left for the next sweep.
    pub(crate) fn sweep(&self, idle_limit: Duration) {
        let now = Instant::now();
        for slot in self.slots() {
            let expired =
                |session: &Session| !session.is_alive() || session.idle_for(now) >= idle_limit;
            let Ok(changing) = Arc::clone(&slot.changing).try_lock_owned() else { continue };
            if slot.current().is_some_and(|session| expired(&session)) {
                let session = slot.take();
                tokio::spawn(async move {
                    let _changing = changing;
                    if let Some(session) = session {
                        session.stop().await;
                    }
                });
            }
        }
    }

    /// Ends every session at once.
    pub(crate) async fn stop_all(&self) {
        let mut stopping = JoinSet::new();
        for slot in self.slots() {
            if let Some(session) = slot.take() {
                stopping.spawn(async move { session.stop().await });
            }
        }
        while stopping.join_next().await.is_some() {}
    }

    fn slot(&self, workspace: &str, server: &str) -> Arc<Slot> {
        let mut slots = lock(&self.slots);
        let slot = slots.entry((workspace.to_owned(), server.to_owned())).or_default();
        Arc::clone(slot)
    }

    fn slots(&self) -> Vec<Arc<Slot>> {
        lock(&self.slots).values().cloned().collect()
    }

    /// `s-<daemon pid>-<n>`: unique while this daemon runs, and across daemons that run at once.
    fn next_id(&self) -> String {
        let number = self.started.fetch_add(1, Ordering::Relaxed) + 1;
        format!("s-{}-{number}", std::process::id())
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
