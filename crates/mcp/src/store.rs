//! The note store: where it lives, opening it without side effects, and its schema migrations.
//!
//! The store is one SQLite file. Reads and `--dry-run` open an existing store with
//! `PRAGMA query_only` and never create it. Only [`migrate`] creates a store, with its directory
//! and schema; `notes create` and `store migrate` run it when there is no store yet. An existing
//! store with pending migrations is upgraded only by `mcp store migrate`, and a SQLite
//! database another program wrote is refused, never changed.

pub mod schema;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use diesel::connection::SimpleConnection;
use diesel::dsl::sql;
use diesel::migration::MigrationSource;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Text};
use diesel::sqlite::Sqlite;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::json;

use crate::NAME;
use crate::failure::{ErrorCode, Failure};

/// The schema migrations in `migrations/`, compiled into the binary.
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// Selects the store when `--store` is absent.
pub const STORE_ENVIRONMENT_VARIABLE: &str = "MCP_STORE";

/// The store's file name inside the data directory.
pub const STORE_FILE: &str = "store.sqlite3";

/// How long a command waits for another process's write lock before failing with
/// `UNAVAILABLE`.
const BUSY_TIMEOUT_MILLISECONDS: u32 = 5_000;

/// Where the store path came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum StoreSource {
    /// `--store`.
    Flag,
    /// The `MCP_STORE` environment variable.
    Environment,
    /// `$XDG_DATA_HOME/mcp/store.sqlite3`, else `~/.local/share/mcp/store.sqlite3`
    /// (`%LOCALAPPDATA%\mcp\store.sqlite3` on Windows).
    Default,
}

/// The store a command targets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreLocation {
    /// Absolute.
    pub path: PathBuf,
    pub source: StoreSource,
}

impl StoreLocation {
    /// `flag`, else the `MCP_STORE` variable `environment` reports, else the default
    /// location. A relative path is relative to `cwd`.
    ///
    /// # Errors
    ///
    /// `INVALID_ARGUMENT` for an empty `--store`, and `PRECONDITION_FAILED` when no default
    /// data directory can be determined.
    pub fn resolve(
        flag: Option<String>,
        cwd: &Path,
        environment: impl Fn(&str) -> Option<OsString>,
    ) -> Result<StoreLocation, Failure> {
        if let Some(flag) = flag {
            if flag.is_empty() {
                return Err(Failure::invalid_argument("--store must name a file")
                    .with_details(json!({ "argument": "store", "value": flag })));
            }
            return Ok(StoreLocation { path: cwd.join(flag), source: StoreSource::Flag });
        }
        if let Some(value) =
            environment(STORE_ENVIRONMENT_VARIABLE).filter(|value| !value.is_empty())
        {
            return Ok(StoreLocation { path: cwd.join(value), source: StoreSource::Environment });
        }
        let directory = data_directory(&environment).ok_or_else(|| {
            Failure::precondition("cannot determine the data directory for the default store")
                .with_details(json!({ "checked": ["XDG_DATA_HOME", "LOCALAPPDATA", "HOME"] }))
                .suggest(format!("Pass --store <path> or set {STORE_ENVIRONMENT_VARIABLE}"))
        })?;
        Ok(StoreLocation {
            path: directory.join(NAME).join(STORE_FILE),
            source: StoreSource::Default,
        })
    }

    pub fn display(&self) -> String {
        self.path.display().to_string()
    }
}

/// The XDG data directory, or Windows' local application data directory.
fn data_directory(environment: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let absolute =
        |key: &str| environment(key).map(PathBuf::from).filter(|path| path.is_absolute());
    absolute("XDG_DATA_HOME")
        .or_else(|| if cfg!(windows) { absolute("LOCALAPPDATA") } else { None })
        .or_else(|| absolute("HOME").map(|home| home.join(".local").join("share")))
}

/// What a store looks like before anything touches it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inspection {
    /// No file, or an empty file: the first write creates the schema.
    Absent,
    /// Current schema, holding `notes` notes.
    Ready { notes: i64 },
    /// An older schema; `mcp store migrate` applies `pending`.
    NeedsMigration { pending: Vec<String> },
    /// Not something this mcp can use, for `problem`.
    Unusable { problem: String },
}

/// Inspects the store without creating or changing anything.
///
/// # Errors
///
/// `UNAVAILABLE` when another process holds the store's lock past the busy timeout.
pub fn inspect(location: &StoreLocation) -> Result<Inspection, Failure> {
    let path = &location.path;
    if !path.exists() {
        return Ok(Inspection::Absent);
    }
    if path.is_dir() {
        return Ok(Inspection::Unusable {
            problem: format!("{} is a directory, not a store file", path.display()),
        });
    }
    let mut connection = match connect(path, true) {
        Ok(connection) => connection,
        Err(problem) => return Ok(Inspection::Unusable { problem }),
    };
    match schema_state(&mut connection) {
        Ok(SchemaState::Empty) => Ok(Inspection::Absent),
        Ok(SchemaState::Current) => {
            let notes = crate::notes::count(&mut connection).map_err(Failure::from)?;
            Ok(Inspection::Ready { notes })
        }
        Ok(SchemaState::Pending(pending)) => Ok(Inspection::NeedsMigration { pending }),
        Ok(SchemaState::Foreign) => Ok(Inspection::Unusable { problem: foreign() }),
        Ok(SchemaState::Newer(unknown)) => Ok(Inspection::Unusable {
            problem: format!(
                "the store has migrations this {NAME} does not know ({}); it was written by a \
                 newer {NAME}",
                unknown.join(", ")
            ),
        }),
        Err(error) => match Failure::from(error) {
            failure if failure.code == ErrorCode::Unavailable => Err(failure),
            failure => Ok(Inspection::Unusable {
                problem: format!("{} is not a {NAME} store: {}", path.display(), failure.message),
            }),
        },
    }
}

/// Opens an existing store for reading. Never creates the file and never writes: the
/// connection runs with `PRAGMA query_only`. `None` when the store does not exist yet.
///
/// # Errors
///
/// `PRECONDITION_FAILED` when the store needs `mcp store migrate` or is unusable, and
/// `UNAVAILABLE` when it is locked.
pub fn open_for_reading(location: &StoreLocation) -> Result<Option<SqliteConnection>, Failure> {
    match inspect(location)? {
        Inspection::Absent => Ok(None),
        Inspection::Ready { .. } => {
            connect(&location.path, true).map(Some).map_err(|problem| unusable(location, &problem))
        }
        Inspection::NeedsMigration { pending } => Err(needs_migration(location, &pending)),
        Inspection::Unusable { problem } => Err(unusable(location, &problem)),
    }
}

/// Opens an existing store for writing. Never creates the store: `None` when it does not exist
/// yet, and [`migrate`] creates it.
///
/// # Errors
///
/// `PRECONDITION_FAILED` when the store needs `mcp store migrate` or is unusable, and
/// `UNAVAILABLE` when it is locked.
pub fn open_for_writing(location: &StoreLocation) -> Result<Option<SqliteConnection>, Failure> {
    match inspect(location)? {
        Inspection::Absent => Ok(None),
        Inspection::Ready { .. } => {
            connect(&location.path, false).map(Some).map_err(|problem| unusable(location, &problem))
        }
        Inspection::NeedsMigration { pending } => Err(needs_migration(location, &pending)),
        Inspection::Unusable { problem } => Err(unusable(location, &problem)),
    }
}

/// What `migrate` would do, without doing it: whether it would create a new store, and the
/// migrations it would apply, oldest first.
///
/// # Errors
///
/// `PRECONDITION_FAILED` when the store is unusable, and `UNAVAILABLE` when it is locked.
pub fn pending_migrations(location: &StoreLocation) -> Result<(bool, Vec<String>), Failure> {
    match inspect(location)? {
        Inspection::Absent => {
            Ok((true, known_migrations().into_iter().map(|(_, name)| name).collect()))
        }
        Inspection::Ready { .. } => Ok((false, Vec::new())),
        Inspection::NeedsMigration { pending } => Ok((false, pending)),
        Inspection::Unusable { problem } => Err(unusable(location, &problem)),
    }
}

/// A store [`migrate`] brought to this binary's schema.
pub struct Migrated {
    pub connection: SqliteConnection,
    /// Whether this call created the store: it had no schema when the call took the store's
    /// write lock, so of two calls racing to create a store, exactly one reports `true`.
    pub created: bool,
    /// The migrations this call applied, oldest first.
    pub applied: Vec<String>,
}

/// Creates the store if needed and applies every pending migration, under the store's write
/// lock.
///
/// # Errors
///
/// `PRECONDITION_FAILED` when the store is unusable or cannot be created, and `UNAVAILABLE`
/// when it is locked.
pub fn migrate(location: &StoreLocation) -> Result<Migrated, Failure> {
    let path = &location.path;
    if let Inspection::Unusable { problem } = inspect(location)? {
        return Err(unusable(location, &problem));
    }
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory).map_err(|error| {
            unusable(location, &format!("cannot create {}: {error}", directory.display()))
        })?;
    }
    let mut connection = connect(path, false).map_err(|problem| unusable(location, &problem))?;
    // The state read inside the write transaction, not the inspection above, decides: another
    // process may have created the store in between.
    let outcome = connection.immediate_transaction(|connection| {
        let (created, pending) = match schema_state(connection)? {
            SchemaState::Empty => {
                (true, known_migrations().into_iter().map(|(_, name)| name).collect())
            }
            SchemaState::Pending(pending) => (false, pending),
            SchemaState::Current => (false, Vec::new()),
            SchemaState::Newer(unknown) => {
                return Err(MigrationError::Failure(unusable(
                    location,
                    &format!("the store has migrations this {NAME} does not know: {unknown:?}"),
                )));
            }
            SchemaState::Foreign => {
                return Err(MigrationError::Failure(unusable(location, &foreign())));
            }
        };
        connection.run_pending_migrations(MIGRATIONS).map_err(MigrationError::Migration)?;
        Ok((created, pending))
    });
    match outcome {
        Ok((created, applied)) => Ok(Migrated { connection, created, applied }),
        Err(MigrationError::Failure(failure)) => Err(failure),
        Err(MigrationError::Store(error)) => Err(error.into()),
        Err(MigrationError::Migration(error)) => match error.downcast::<diesel::result::Error>() {
            Ok(error) => Err((*error).into()),
            Err(error) => Err(Failure::internal(format!("a migration failed: {error}"))),
        },
    }
}

enum MigrationError {
    Store(diesel::result::Error),
    Migration(Box<dyn std::error::Error + Send + Sync>),
    Failure(Failure),
}

impl From<diesel::result::Error> for MigrationError {
    fn from(error: diesel::result::Error) -> MigrationError {
        MigrationError::Store(error)
    }
}

/// Where the store's schema stands relative to this binary's migrations.
enum SchemaState {
    /// No tables, views, indexes, or triggers at all: a new, empty file.
    Empty,
    Current,
    /// Migrations this binary has that the store lacks, by name.
    Pending(Vec<String>),
    /// Versions the store has that this binary does not know.
    Newer(Vec<String>),
    /// Tables or other schema objects without a single applied migration: a database another
    /// program wrote, which migrating would mix this binary's tables into.
    Foreign,
}

#[derive(QueryableByName)]
struct AppliedVersion {
    #[diesel(sql_type = Text)]
    version: String,
}

/// Compares the store's applied migrations with this binary's, with reads only:
/// diesel's own `has_pending_migration` creates its bookkeeping table, which a read must not.
fn schema_state(connection: &mut SqliteConnection) -> QueryResult<SchemaState> {
    let objects: i64 = diesel::select(sql::<BigInt>(
        "(SELECT COUNT(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' \
         AND name <> '__diesel_schema_migrations')",
    ))
    .get_result(connection)?;
    let tracked: bool = diesel::select(sql::<Bool>(
        "EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'table' \
         AND name = '__diesel_schema_migrations')",
    ))
    .get_result(connection)?;
    if !tracked && objects == 0 {
        return Ok(SchemaState::Empty);
    }
    let applied: Vec<String> = if tracked {
        diesel::sql_query("SELECT version FROM __diesel_schema_migrations")
            .load::<AppliedVersion>(connection)?
            .into_iter()
            .map(|row| row.version)
            .collect()
    } else {
        Vec::new()
    };
    if applied.is_empty() && objects > 0 {
        return Ok(SchemaState::Foreign);
    }
    let known = known_migrations();
    let newer: Vec<String> = applied
        .iter()
        .filter(|version| !known.iter().any(|(known, _)| known == *version))
        .cloned()
        .collect();
    if !newer.is_empty() {
        return Ok(SchemaState::Newer(newer));
    }
    let pending: Vec<String> = known
        .into_iter()
        .filter(|(version, _)| !applied.contains(version))
        .map(|(_, name)| name)
        .collect();
    Ok(if pending.is_empty() { SchemaState::Current } else { SchemaState::Pending(pending) })
}

/// This binary's migrations as `(version, name)`, oldest first.
fn known_migrations() -> Vec<(String, String)> {
    let migrations = MigrationSource::<Sqlite>::migrations(&MIGRATIONS)
        .expect("embedded migrations are compiled into the binary");
    let mut known: Vec<(String, String)> = migrations
        .iter()
        .map(|migration| (migration.name().version().to_string(), migration.name().to_string()))
        .collect();
    known.sort();
    known
}

/// Opens the file at `path`, which must exist unless `read_only` is false.
fn connect(path: &Path, read_only: bool) -> Result<SqliteConnection, String> {
    let url = path.to_str().ok_or_else(|| format!("{} is not a UTF-8 path", path.display()))?;
    let mut connection = SqliteConnection::establish(url).map_err(|error| error.to_string())?;
    let mut pragmas = format!("PRAGMA busy_timeout = {BUSY_TIMEOUT_MILLISECONDS};");
    if read_only {
        pragmas.push_str("PRAGMA query_only = ON;");
    }
    connection.batch_execute(&pragmas).map_err(|error| error.to_string())?;
    Ok(connection)
}

fn needs_migration(location: &StoreLocation, pending: &[String]) -> Failure {
    Failure::precondition(format!(
        "the store at {} uses an older schema; `{NAME} store migrate` upgrades it",
        location.display()
    ))
    .with_details(json!({ "store": location.display(), "pendingMigrations": pending }))
    .suggest(format!("{NAME} store migrate --dry-run"))
    .suggest(format!("{NAME} store migrate"))
}

fn unusable(location: &StoreLocation, problem: &str) -> Failure {
    Failure::precondition(format!("cannot use the store at {}: {problem}", location.display()))
        .with_details(json!({ "store": location.display(), "source": location.source }))
        .suggest(format!("{NAME} context show"))
        .suggest("Pass --store <path> to use another store")
}

/// The problem with a database another program wrote.
fn foreign() -> String {
    format!(
        "it is a SQLite database with no {NAME} migration history, written by another program; \
         {NAME} leaves it unchanged"
    )
}

impl From<diesel::result::Error> for Failure {
    fn from(error: diesel::result::Error) -> Failure {
        let message = error.to_string();
        if message.contains("database is locked") || message.contains("database is busy") {
            return Failure::new(
                ErrorCode::Unavailable,
                "another process holds the store's write lock",
            )
            .with_details(json!({ "waitedMilliseconds": BUSY_TIMEOUT_MILLISECONDS }))
            .suggest("Retry the same command");
        }
        if message.contains("not a database") || message.contains("malformed") {
            return Failure::precondition(message);
        }
        Failure::internal(format!("the store failed: {message}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let pairs: Vec<(String, OsString)> =
            pairs.iter().map(|(key, value)| ((*key).to_string(), OsString::from(value))).collect();
        move |key| pairs.iter().find(|(name, _)| name == key).map(|(_, value)| value.clone())
    }

    fn absolute(path: &str) -> PathBuf {
        if cfg!(windows) { PathBuf::from(format!("C:{path}")) } else { PathBuf::from(path) }
    }

    #[test]
    fn the_flag_wins_then_the_variable_then_the_default() {
        let cwd = absolute("/work");
        let home = absolute("/home/me");
        let both = environment(&[
            (STORE_ENVIRONMENT_VARIABLE, "from-env.sqlite3"),
            ("HOME", home.to_str().unwrap()),
        ]);
        let flag = StoreLocation::resolve(Some("a.sqlite3".into()), &cwd, &both).unwrap();
        assert_eq!(flag, StoreLocation { path: cwd.join("a.sqlite3"), source: StoreSource::Flag });
        let variable = StoreLocation::resolve(None, &cwd, &both).unwrap();
        assert_eq!(variable.path, cwd.join("from-env.sqlite3"));
        assert_eq!(variable.source, StoreSource::Environment);
        let default =
            StoreLocation::resolve(None, &cwd, environment(&[("HOME", home.to_str().unwrap())]))
                .unwrap();
        assert_eq!(default.source, StoreSource::Default);
        if !cfg!(windows) {
            assert_eq!(default.path, home.join(".local/share").join(NAME).join(STORE_FILE));
        }
    }

    #[test]
    fn xdg_data_home_must_be_absolute_to_count() {
        let cwd = absolute("/work");
        let data = absolute("/data");
        let home = absolute("/home/me");
        let xdg = environment(&[
            ("XDG_DATA_HOME", data.to_str().unwrap()),
            ("HOME", home.to_str().unwrap()),
        ]);
        assert_eq!(
            StoreLocation::resolve(None, &cwd, xdg).unwrap().path,
            data.join(NAME).join(STORE_FILE)
        );
        let relative = environment(&[("XDG_DATA_HOME", "data"), ("HOME", home.to_str().unwrap())]);
        assert!(StoreLocation::resolve(None, &cwd, relative).unwrap().path.starts_with(&home));
    }

    #[test]
    fn an_empty_variable_is_unset_but_an_empty_flag_is_an_error() {
        let cwd = absolute("/work");
        let home = absolute("/home/me");
        let empty =
            environment(&[(STORE_ENVIRONMENT_VARIABLE, ""), ("HOME", home.to_str().unwrap())]);
        assert_eq!(
            StoreLocation::resolve(None, &cwd, &empty).unwrap().source,
            StoreSource::Default
        );
        let failure = StoreLocation::resolve(Some(String::new()), &cwd, &empty).unwrap_err();
        assert_eq!(failure.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn without_any_data_directory_the_default_is_a_precondition_failure() {
        let failure =
            StoreLocation::resolve(None, &absolute("/work"), environment(&[])).unwrap_err();
        assert_eq!(failure.code, ErrorCode::PreconditionFailed);
        assert!(failure.suggestions[0].contains("--store"));
    }

    #[test]
    fn every_migration_applies_and_reverts() {
        let mut connection = SqliteConnection::establish(":memory:").unwrap();
        assert!(matches!(schema_state(&mut connection).unwrap(), SchemaState::Empty));
        connection.run_pending_migrations(MIGRATIONS).unwrap();
        assert!(matches!(schema_state(&mut connection).unwrap(), SchemaState::Current));
        connection.revert_all_migrations(MIGRATIONS).unwrap();
        assert!(matches!(schema_state(&mut connection).unwrap(), SchemaState::Pending(_)));
    }

    #[test]
    fn of_two_migrations_racing_to_create_a_store_exactly_one_creates_it() {
        for _ in 0..5 {
            let directory = tempfile::tempdir().unwrap();
            let location = StoreLocation {
                path: directory.path().join(STORE_FILE),
                source: StoreSource::Flag,
            };
            let start = std::sync::Barrier::new(2);
            let created: Vec<bool> = std::thread::scope(|scope| {
                let racers: Vec<_> = (0..2)
                    .map(|_| {
                        scope.spawn(|| {
                            start.wait();
                            migrate(&location).map(|migrated| migrated.created)
                        })
                    })
                    .collect();
                racers.into_iter().map(|racer| racer.join().unwrap().unwrap()).collect()
            });
            assert_eq!(created.iter().filter(|created| **created).count(), 1, "{created:?}");
        }
    }
}
