//! The deterministic MCP server that `mcp`'s process tests and registered examples run against.
//! It is an example of the `mcp` package so `cargo test -p mcp` builds it; it never ships.
//!
//! By default it serves MCP over stdio. `--http <addr>` serves Streamable HTTP at `/mcp` instead
//! and prints `http://<bound-addr>/mcp` as the only line on stdout, so `--http 127.0.0.1:0` lets
//! the caller learn the port.

mod server;
mod tools;

use std::{
    ffi::OsString,
    io::{self, Write},
    process::ExitCode,
    sync::Arc,
};

use axum::{
    Router,
    extract::{Request, State},
    http::{HeaderName, HeaderValue, StatusCode, header::WWW_AUTHENTICATE},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use rmcp::{
    ServiceExt,
    transport::{
        stdio,
        streamable_http_server::{
            StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
        },
    },
};
use tokio::net::TcpListener;

use crate::server::TestServer;

const USAGE: &str = "usage: mcp-test-server [--http <addr>] [--require-header <Name>=<Value>]... \
                     [--instructions <text>] [--no-tools] [--stderr-lines <n>] \
                     [--fail-start <message>]";

/// Headers every HTTP request must carry, each with exactly this value.
type RequiredHeaders = Arc<Vec<(HeaderName, HeaderValue)>>;

struct Options {
    /// Serve Streamable HTTP on this address instead of stdio.
    http: Option<String>,
    required_headers: Vec<(HeaderName, HeaderValue)>,
    /// Write this to stderr and exit 1 instead of serving.
    fail_start: Option<String>,
    /// Write `stderr line 1` to `stderr line <n>` at startup.
    stderr_lines: u32,
    tools: bool,
    instructions: String,
}

impl Options {
    fn parse(arguments: Vec<OsString>) -> Result<Self, String> {
        let mut options = Self {
            http: None,
            required_headers: Vec::new(),
            fail_start: None,
            stderr_lines: 0,
            tools: true,
            instructions: "Test server for mcp.".to_owned(),
        };
        let mut arguments = arguments.into_iter().map(|argument| {
            argument
                .into_string()
                .map_err(|argument| format!("{} is not UTF-8", argument.display()))
        });
        while let Some(flag) = arguments.next().transpose()? {
            let mut value =
                || arguments.next().unwrap_or_else(|| Err(format!("{flag} needs a value")));
            match flag.as_str() {
                "--http" => options.http = Some(value()?),
                "--require-header" => options.required_headers.push(required_header(&value()?)?),
                "--fail-start" => options.fail_start = Some(value()?),
                "--stderr-lines" => {
                    let lines = value()?;
                    options.stderr_lines = lines
                        .parse()
                        .map_err(|error| format!("--stderr-lines {lines}: {error}"))?;
                }
                "--no-tools" => options.tools = false,
                "--instructions" => options.instructions = value()?,
                _ => return Err(format!("unknown argument {flag}")),
            }
        }
        if options.http.is_none() && !options.required_headers.is_empty() {
            return Err("--require-header needs --http".to_owned());
        }
        Ok(options)
    }
}

fn required_header(argument: &str) -> Result<(HeaderName, HeaderValue), String> {
    let invalid = |reason: &dyn std::fmt::Display| format!("--require-header {argument}: {reason}");
    let (name, value) =
        argument.split_once('=').ok_or_else(|| invalid(&"expected <Name>=<Value>"))?;
    let name = HeaderName::try_from(name).map_err(|error| invalid(&error))?;
    let value = HeaderValue::try_from(value).map_err(|error| invalid(&error))?;
    Ok((name, value))
}

fn main() -> ExitCode {
    let options = match Options::parse(std::env::args_os().skip(1).collect()) {
        Ok(options) => options,
        Err(message) => {
            report(&format!("{message}\n{USAGE}"));
            return ExitCode::from(2);
        }
    };
    if let Some(message) = &options.fail_start {
        report(message);
        return ExitCode::FAILURE;
    }
    match run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            report(&message);
            ExitCode::FAILURE
        }
    }
}

fn run(options: Options) -> Result<(), String> {
    let mut stderr = io::stderr().lock();
    for line in 1..=options.stderr_lines {
        writeln!(stderr, "stderr line {line}")
            .map_err(|error| format!("cannot write to stderr: {error}"))?;
    }
    drop(stderr);
    let server = TestServer::new(options.tools, options.instructions);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("cannot start the async runtime: {error}"))?;
    match options.http {
        Some(address) => runtime.block_on(serve_http(server, &address, options.required_headers)),
        None => runtime.block_on(serve_stdio(server)),
    }
}

async fn serve_stdio(server: TestServer) -> Result<(), String> {
    let service = server
        .serve(stdio())
        .await
        .map_err(|error| format!("MCP initialization failed: {error}"))?;
    service.waiting().await.map_err(|error| format!("the MCP service failed: {error}"))?;
    Ok(())
}

async fn serve_http(
    server: TestServer,
    address: &str,
    required_headers: Vec<(HeaderName, HeaderValue)>,
) -> Result<(), String> {
    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let router = Router::new()
        .route_service("/mcp", service)
        .layer(middleware::from_fn_with_state(Arc::new(required_headers), require_headers));
    let listener = TcpListener::bind(address)
        .await
        .map_err(|error| format!("cannot listen on {address}: {error}"))?;
    let bound =
        listener.local_addr().map_err(|error| format!("cannot read the address: {error}"))?;
    let mut stdout = io::stdout();
    writeln!(stdout, "http://{bound}/mcp")
        .and_then(|()| stdout.flush())
        .map_err(|error| format!("cannot write to stdout: {error}"))?;
    axum::serve(listener, router).await.map_err(|error| format!("the HTTP server failed: {error}"))
}

/// Answers 401 with a Bearer challenge, and no MCP body, unless every required header was sent.
async fn require_headers(
    State(required): State<RequiredHeaders>,
    request: Request,
    next: Next,
) -> Response {
    let headers = request.headers();
    let authorized =
        required.iter().all(|(name, value)| headers.get_all(name).iter().any(|sent| sent == value));
    if authorized {
        next.run(request).await
    } else {
        (StatusCode::UNAUTHORIZED, [(WWW_AUTHENTICATE, "Bearer")]).into_response()
    }
}

/// Writes `message` to stderr; there is nowhere left to report a failure to.
fn report(message: &str) {
    let _ignored = writeln!(io::stderr(), "{message}");
}
