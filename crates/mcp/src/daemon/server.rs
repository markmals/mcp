//! The daemon process: `mcp __daemon <socket>`.

use std::fs::{self, File, OpenOptions, Permissions, TryLockError};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::UnixListener;
use tokio::signal::unix::{Signal, SignalKind, signal};

use super::connection;
use super::dispatch::Daemon;
use super::paths::DaemonPaths;

/// A session without a request for this long ends.
const SESSION_IDLE: Duration = Duration::from_secs(30 * 60);
/// The daemon exits once it has held no session and served no request for this long.
const DAEMON_IDLE: Duration = Duration::from_secs(60);
/// How often the daemon looks for expired sessions and for its own idleness.
const SWEEP_INTERVAL: Duration = Duration::from_secs(1);
/// How long a daemon waits for an exiting one to release the pid file before leaving it be.
const TAKEOVER_WAIT: Duration = Duration::from_secs(2);
/// How long ending every session may take when the daemon exits.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(6);

/// The daemon process's whole life; returns its exit status. A daemon of this version already
/// running makes this one exit at once with status 0. Writes nothing to stdout or stderr.
pub fn run(socket: PathBuf) -> u8 {
    let Some(paths) = DaemonPaths::for_socket(&socket) else { return 1 };
    if paths.ensure_directory().is_err() {
        return 1;
    }
    let pid_file = match lock_pid_file(&paths.pid_file) {
        Ok(Some(file)) => file,
        Ok(None) => return 0,
        Err(_) => return 1,
    };
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread().enable_all().build() else {
        return 1;
    };
    let status = runtime.block_on(serve(&paths.socket, pid_file));
    runtime.shutdown_timeout(Duration::from_secs(1));
    status
}

/// Locks the pid file for the daemon's life and writes the daemon's pid into it. `None` when
/// another daemon holds it; one that is exiting gets [`TAKEOVER_WAIT`] to let go.
fn lock_pid_file(path: &Path) -> io::Result<Option<File>> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    let deadline = Instant::now() + TAKEOVER_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Error(error)) => return Err(error),
        }
    }
    file.set_len(0)?;
    writeln!(file, "{}", std::process::id())?;
    Ok(Some(file))
}

/// Serves until a stop signal or [`DAEMON_IDLE`]. `pid_file` is released once the socket is
/// gone, so a new daemon can start while this one still ends its sessions.
async fn serve(socket: &Path, pid_file: File) -> u8 {
    let Ok(listener) = bind(socket) else { return 1 };
    let Ok(mut stop) = StopSignals::register() else { return 1 };
    let daemon = Arc::new(Daemon::default());
    let mut sweep = tokio::time::interval(SWEEP_INTERVAL);
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    let daemon = Arc::clone(&daemon);
                    tokio::spawn(async move { connection::serve(&daemon, stream).await });
                }
                // Out of file descriptors, say: let running requests finish and free some.
                Err(_) => tokio::time::sleep(SWEEP_INTERVAL).await,
            },
            () = stop.received() => break,
            _ = sweep.tick() => {
                daemon.sessions.sweep(SESSION_IDLE);
                if daemon.idle_for() >= DAEMON_IDLE {
                    break;
                }
            }
        }
    }
    drop(listener);
    // Best effort: a stale socket left behind is replaced by the next daemon.
    let _ = fs::remove_file(socket);
    drop(pid_file);
    // Sessions still ending after the grace period are dropped, which kills their servers.
    let _ = tokio::time::timeout(SHUTDOWN_GRACE, daemon.sessions.stop_all()).await;
    0
}

/// Replaces any socket a daemon that no longer runs left behind; only the user can connect.
fn bind(socket: &Path) -> io::Result<UnixListener> {
    match fs::remove_file(socket) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(socket)?;
    fs::set_permissions(socket, Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// `SIGTERM` and `SIGINT` end the daemon. `SIGHUP`, which a closing terminal can send, does not.
struct StopSignals {
    terminate: Signal,
    interrupt: Signal,
    hangup: Signal,
}

impl StopSignals {
    fn register() -> io::Result<StopSignals> {
        Ok(StopSignals {
            terminate: signal(SignalKind::terminate())?,
            interrupt: signal(SignalKind::interrupt())?,
            hangup: signal(SignalKind::hangup())?,
        })
    }

    async fn received(&mut self) {
        loop {
            tokio::select! {
                _ = self.terminate.recv() => return,
                _ = self.interrupt.recv() => return,
                _ = self.hangup.recv() => {}
            }
        }
    }
}
