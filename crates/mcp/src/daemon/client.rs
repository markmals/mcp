//! The `mcp` side of the socket: synchronous, one connection per request.

use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::failures::describe_duration;
use super::paths::DaemonPaths;
use super::protocol::{DaemonStatus, Reply, ReplyBody, Request};
use crate::failure::{ErrorCode, Failure};

/// How long a client waits for a daemon it started to accept connections.
const START_WAIT: Duration = Duration::from_secs(10);
const START_POLL: Duration = Duration::from_millis(20);
/// How long [`status`] waits for a running daemon's answer.
const STATUS_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub enum DaemonState {
    Running(DaemonStatus),
    Stopped,
}

/// Whether this version's daemon is running, and its sessions. Never starts the daemon: a socket
/// nobody answers is [`DaemonState::Stopped`].
///
/// # Errors
///
/// `PRECONDITION_FAILED` when the daemon's directory is not the user's own; `TIMEOUT` when a
/// daemon accepts the connection but does not answer.
pub fn status(paths: &DaemonPaths) -> Result<DaemonState, Failure> {
    if !paths.directory.exists() {
        return Ok(DaemonState::Stopped);
    }
    paths.check_directory()?;
    let Ok(stream) = UnixStream::connect(&paths.socket) else {
        return Ok(DaemonState::Stopped);
    };
    match exchange(&stream, &Request::Status, STATUS_WAIT) {
        Ok(ReplyBody::Status(status)) => Ok(DaemonState::Running(status)),
        Ok(_) => Err(Failure::internal("the session daemon answered a status request wrongly")),
        Err(Exchange::Failed(failure)) => Err(failure),
        // A daemon that closes the connection unanswered is exiting.
        Err(Exchange::Lost(_)) => Ok(DaemonState::Stopped),
    }
}

/// Sends `request`, starting the daemon first when none answers, and waits at most `wait` for
/// the reply.
///
/// # Errors
///
/// The daemon's failure for the request; `UNAVAILABLE` when the daemon cannot be started or
/// reached or the connection is lost; `TIMEOUT` when no reply arrives within `wait`.
pub fn send(paths: &DaemonPaths, request: &Request, wait: Duration) -> Result<ReplyBody, Failure> {
    paths.ensure_directory()?;
    let stream = match UnixStream::connect(&paths.socket) {
        Ok(stream) => stream,
        // The only connect failure no daemon can fix: a path longer than a socket address holds.
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
            return Err(Failure::precondition(format!(
                "the session daemon's socket path {} cannot be used: {error}",
                paths.socket.display()
            ))
            .suggest("set XDG_RUNTIME_DIR to a shorter absolute directory"));
        }
        Err(_) => start(paths)?,
    };
    exchange(&stream, request, wait).map_err(Exchange::into_failure)
}

/// Sends `request` only to a daemon that is already running; `Ok(None)` when none is.
///
/// # Errors
///
/// As [`send`], except that no daemon is ever started.
pub fn send_if_running(
    paths: &DaemonPaths,
    request: &Request,
    wait: Duration,
) -> Result<Option<ReplyBody>, Failure> {
    if !paths.directory.exists() {
        return Ok(None);
    }
    paths.check_directory()?;
    let Ok(stream) = UnixStream::connect(&paths.socket) else {
        return Ok(None);
    };
    exchange(&stream, request, wait).map(Some).map_err(Exchange::into_failure)
}

/// Starts the daemon unless another client started it meanwhile, and connects to it.
fn start(paths: &DaemonPaths) -> Result<UnixStream, Failure> {
    let _lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&paths.start_lock)
        .and_then(|file| file.lock().map(|()| file))
        .map_err(|error| {
            not_started(&format!("cannot lock {}: {error}", paths.start_lock.display()))
        })?;
    if let Ok(stream) = UnixStream::connect(&paths.socket) {
        return Ok(stream);
    }
    let executable = std::env::current_exe()
        .map_err(|error| not_started(&format!("cannot find the mcp executable: {error}")))?;
    // Its own process group keeps a terminal's Ctrl-C to this command from reaching the daemon.
    // The daemon outlives this process, which never waits for it unless it exits at once.
    let mut daemon = Command::new(executable)
        .arg("__daemon")
        .arg(&paths.socket)
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|error| not_started(&format!("cannot run the mcp executable: {error}")))?;
    let deadline = Instant::now() + START_WAIT;
    loop {
        std::thread::sleep(START_POLL);
        if let Ok(stream) = UnixStream::connect(&paths.socket) {
            return Ok(stream);
        }
        if let Ok(Some(status)) = daemon.try_wait() {
            return Err(not_started(&format!("it exited ({status})")));
        }
        if Instant::now() >= deadline {
            return Err(not_started(&format!(
                "nothing answered at {} within {}",
                paths.socket.display(),
                describe_duration(START_WAIT)
            )));
        }
    }
}

fn not_started(cause: &str) -> Failure {
    Failure::unavailable(format!("the session daemon did not start: {cause}"))
}

enum Exchange {
    Failed(Failure),
    /// The daemon closed the connection, or it broke, before a whole reply arrived.
    Lost(io::Error),
}

impl Exchange {
    fn into_failure(self) -> Failure {
        match self {
            Exchange::Failed(failure) => failure,
            Exchange::Lost(error) => Failure::unavailable(format!(
                "lost the connection to the session daemon before it answered: {error}"
            )),
        }
    }
}

/// Writes `request` as one line and reads one reply line. The write half stays open: closing it
/// would tell the daemon to cancel the request.
fn exchange(stream: &UnixStream, request: &Request, wait: Duration) -> Result<ReplyBody, Exchange> {
    let mut line = serde_json::to_vec(request).map_err(|error| {
        Exchange::Failed(Failure::internal(format!("cannot encode a daemon request: {error}")))
    })?;
    line.push(b'\n');
    let mut writer = stream;
    writer.write_all(&line).map_err(Exchange::Lost)?;
    let reply = read_line(stream, wait)?;
    match serde_json::from_slice::<Reply>(&reply) {
        Ok(Reply::Ok(body)) => Ok(body),
        Ok(Reply::Error(error)) => Err(Exchange::Failed(Failure {
            code: error.code,
            message: error.message,
            details: Box::new(error.details),
            suggestions: error.suggestions,
        })),
        Err(error) => Err(Exchange::Failed(Failure::internal(format!(
            "the session daemon's reply is not valid: {error}"
        )))),
    }
}

/// One line, without its newline, read within `wait` in total.
fn read_line(stream: &UnixStream, wait: Duration) -> Result<Vec<u8>, Exchange> {
    let deadline = Instant::now() + wait;
    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Exchange::Failed(no_answer(wait)));
        }
        stream.set_read_timeout(Some(remaining)).map_err(Exchange::Lost)?;
        let available = match reader.fill_buf() {
            Ok([]) => {
                return Err(Exchange::Lost(io::Error::from(io::ErrorKind::UnexpectedEof)));
            }
            Ok(available) => available,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error)
                if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) =>
            {
                return Err(Exchange::Failed(no_answer(wait)));
            }
            Err(error) => return Err(Exchange::Lost(error)),
        };
        if let Some(end) = available.iter().position(|&byte| byte == b'\n') {
            line.extend_from_slice(&available[..end]);
            return Ok(line);
        }
        line.extend_from_slice(available);
        let consumed = available.len();
        reader.consume(consumed);
    }
}

fn no_answer(wait: Duration) -> Failure {
    Failure::new(
        ErrorCode::Timeout,
        format!("no answer from the session daemon within {}", describe_duration(wait)),
    )
}
