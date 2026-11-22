//! Notes, the example domain: validation, change planning, and the queries the commands share.
//!
//! Every modifying command plans its [`Change`] from the note as stored, then either returns
//! the plan (`--dry-run`) or applies it inside the same write transaction that read the note.
//! A dry run and a real run therefore make the same decision from the same rules.

use diesel::prelude::*;
use jiff::{Timestamp, Unit};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::json;

use crate::NAME;
use crate::failure::Failure;
use crate::store::schema::notes;

/// The longest title, in characters.
pub const TITLE_MAX_LENGTH: usize = 200;

/// The longest body, in bytes of UTF-8.
pub const BODY_MAX_BYTES: usize = 65_536;

/// One note as stored.
#[derive(Clone, Debug, PartialEq, Eq, Queryable, Selectable, Insertable, Serialize, JsonSchema)]
#[diesel(table_name = notes, check_for_backend(diesel::sqlite::Sqlite))]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// Stable identifier: lowercase letters and digits in hyphen-separated words.
    pub name: String,
    /// One line.
    pub title: String,
    /// Free text; empty when the note has none.
    pub body: String,
    /// Starts at 1 and increases on every change. Pass it to `--expect-revision` to change
    /// only the version you read.
    pub revision: i64,
    /// When the note was created: RFC 3339, UTC, millisecond precision.
    pub created_at: String,
    /// When the note last changed: RFC 3339, UTC, millisecond precision.
    pub updated_at: String,
}

/// The current time as notes record it.
pub fn now() -> String {
    let now = Timestamp::now();
    now.round(Unit::Millisecond).unwrap_or(now).strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// # Errors
///
/// `INVALID_ARGUMENT` for an empty, multi-line, or overlong title.
pub fn validate_title(title: &str) -> Result<(), Failure> {
    let problem = if title.trim().is_empty() {
        Some("--title must not be empty")
    } else if title.contains(['\n', '\r']) {
        Some("--title must be one line")
    } else if title.chars().count() > TITLE_MAX_LENGTH {
        Some("--title is too long")
    } else {
        None
    };
    match problem {
        Some(message) => Err(Failure::invalid_argument(message).with_details(json!({
            "argument": "title",
            "maxLength": TITLE_MAX_LENGTH,
        }))),
        None => Ok(()),
    }
}

/// # Errors
///
/// `INVALID_ARGUMENT` for a body over [`BODY_MAX_BYTES`].
pub fn validate_body(body: &str) -> Result<(), Failure> {
    if body.len() > BODY_MAX_BYTES {
        return Err(Failure::invalid_argument("--body is too long").with_details(json!({
            "argument": "body",
            "maxBytes": BODY_MAX_BYTES,
            "bytes": body.len(),
        })));
    }
    Ok(())
}

/// What a modifying command does to the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Create(Note),
    Update {
        before: Note,
        after: Note,
        changed: Vec<&'static str>,
    },
    /// An update whose values equal the stored ones: nothing to write.
    Unchanged(Note),
    Delete(Note),
}

/// Plans `notes create`.
///
/// # Errors
///
/// `CONFLICT` when a note named `name` exists.
pub fn plan_create(
    existing: Option<Note>,
    name: &str,
    title: String,
    body: String,
    now: &str,
) -> Result<Change, Failure> {
    if let Some(existing) = existing {
        return Err(Failure::conflict(format!("a note named `{name}` already exists"))
            .with_details(json!({ "name": name, "revision": existing.revision }))
            .suggest(format!("{NAME} notes get {name}"))
            .suggest(format!(
                "{NAME} notes update {name} --expect-revision {} --title <title>",
                existing.revision
            ))
            .suggest("Choose another --name"));
    }
    Ok(Change::Create(Note {
        name: name.to_string(),
        title,
        body,
        revision: 1,
        created_at: now.to_string(),
        updated_at: now.to_string(),
    }))
}

/// Plans `notes update`. Fields left `None` keep their stored values.
///
/// # Errors
///
/// `NOT_FOUND` when no note is named `name`, and `CONFLICT` when its revision is not
/// `expected_revision`.
pub fn plan_update(
    existing: Option<Note>,
    name: &str,
    title: Option<String>,
    body: Option<String>,
    expected_revision: Option<u64>,
    now: &str,
) -> Result<Change, Failure> {
    let before = found(existing, name)?;
    check_revision(&before, expected_revision)?;
    let mut after = before.clone();
    let mut changed = Vec::new();
    if let Some(title) = title.filter(|title| *title != before.title) {
        after.title = title;
        changed.push("title");
    }
    if let Some(body) = body.filter(|body| *body != before.body) {
        after.body = body;
        changed.push("body");
    }
    if changed.is_empty() {
        return Ok(Change::Unchanged(before));
    }
    after.revision = before.revision + 1;
    after.updated_at = now.to_string();
    Ok(Change::Update { before, after, changed })
}

/// Plans `notes delete`.
///
/// # Errors
///
/// `NOT_FOUND` when no note is named `name`, and `CONFLICT` when its revision is not
/// `expected_revision`.
pub fn plan_delete(
    existing: Option<Note>,
    name: &str,
    expected_revision: Option<u64>,
) -> Result<Change, Failure> {
    let note = found(existing, name)?;
    check_revision(&note, expected_revision)?;
    Ok(Change::Delete(note))
}

fn found(existing: Option<Note>, name: &str) -> Result<Note, Failure> {
    existing.ok_or_else(|| missing(name))
}

/// The failure for a note that does not exist.
pub fn missing(name: &str) -> Failure {
    Failure::not_found(format!("no note named `{name}`"))
        .with_details(json!({ "name": name }))
        .suggest(format!("{NAME} notes list"))
        .suggest(format!("{NAME} context show"))
}

fn check_revision(note: &Note, expected: Option<u64>) -> Result<(), Failure> {
    let Some(expected) = expected else {
        return Ok(());
    };
    if i64::try_from(expected).is_ok_and(|expected| expected == note.revision) {
        return Ok(());
    }
    Err(Failure::conflict(format!(
        "note `{}` is at revision {}, not {expected}",
        note.name, note.revision
    ))
    .with_details(json!({
        "name": note.name,
        "expectedRevision": expected,
        "actualRevision": note.revision,
    }))
    .suggest(format!("{NAME} notes get {}", note.name))
    .suggest("Read the note again, then retry with --expect-revision set to its revision"))
}

/// Writes `change`.
///
/// # Errors
///
/// When the store rejects the write.
pub fn apply(connection: &mut SqliteConnection, change: &Change) -> QueryResult<()> {
    match change {
        Change::Create(note) => {
            diesel::insert_into(notes::table).values(note).execute(connection)?;
        }
        Change::Update { after, .. } => {
            diesel::update(notes::table.find(&after.name))
                .set((
                    notes::title.eq(&after.title),
                    notes::body.eq(&after.body),
                    notes::revision.eq(after.revision),
                    notes::updated_at.eq(&after.updated_at),
                ))
                .execute(connection)?;
        }
        Change::Unchanged(_) => {}
        Change::Delete(note) => {
            diesel::delete(notes::table.find(&note.name)).execute(connection)?;
        }
    }
    Ok(())
}

/// The note named `name`, if any.
///
/// # Errors
///
/// When the store cannot be read.
pub fn find(connection: &mut SqliteConnection, name: &str) -> QueryResult<Option<Note>> {
    notes::table.find(name).select(Note::as_select()).first(connection).optional()
}

/// Up to `limit` notes after `after` in name order, and whether more follow.
///
/// # Errors
///
/// When the store cannot be read.
pub fn page(
    connection: &mut SqliteConnection,
    after: Option<&str>,
    limit: i64,
) -> QueryResult<(Vec<Note>, bool)> {
    let mut query = notes::table
        .select(Note::as_select())
        .order(notes::name.asc())
        .limit(limit + 1)
        .into_boxed();
    if let Some(after) = after {
        query = query.filter(notes::name.gt(after));
    }
    let mut items = query.load(connection)?;
    let has_more = items.len() > usize::try_from(limit).unwrap_or(usize::MAX);
    items.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    Ok((items, has_more))
}

/// How many notes the store holds.
///
/// # Errors
///
/// When the store cannot be read.
pub fn count(connection: &mut SqliteConnection) -> QueryResult<i64> {
    notes::table.count().get_result(connection)
}

#[cfg(test)]
mod tests {
    use diesel_migrations::MigrationHarness;

    use super::*;
    use crate::failure::ErrorCode;
    use crate::store::MIGRATIONS;

    fn note(name: &str) -> Note {
        match plan_create(None, name, "Title".into(), String::new(), "2026-01-01T00:00:00.000Z") {
            Ok(Change::Create(note)) => note,
            other => panic!("{other:?}"),
        }
    }

    fn store() -> SqliteConnection {
        let mut connection = SqliteConnection::establish(":memory:").unwrap();
        connection.run_pending_migrations(MIGRATIONS).unwrap();
        connection
    }

    #[test]
    fn creating_a_taken_name_conflicts() {
        let failure =
            plan_create(Some(note("groceries")), "groceries", "T".into(), String::new(), "now")
                .unwrap_err();
        assert_eq!(failure.code, ErrorCode::Conflict);
        assert_eq!(failure.details["revision"], 1);
    }

    #[test]
    fn an_update_to_the_same_values_changes_nothing() {
        let stored = note("groceries");
        let change = plan_update(
            Some(stored.clone()),
            "groceries",
            Some("Title".into()),
            None,
            None,
            "later",
        )
        .unwrap();
        assert_eq!(change, Change::Unchanged(stored));
    }

    #[test]
    fn an_update_bumps_the_revision_and_names_the_changed_fields() {
        let change = plan_update(
            Some(note("groceries")),
            "groceries",
            Some("New".into()),
            Some("Milk".into()),
            Some(1),
            "later",
        )
        .unwrap();
        let Change::Update { after, changed, .. } = change else { panic!("{change:?}") };
        assert_eq!(after.revision, 2);
        assert_eq!(after.updated_at, "later");
        assert_eq!(after.created_at, "2026-01-01T00:00:00.000Z");
        assert_eq!(changed, ["title", "body"]);
    }

    #[test]
    fn a_stale_revision_conflicts_and_reports_the_actual_one() {
        for failure in [
            plan_update(Some(note("a")), "a", Some("x".into()), None, Some(7), "now").unwrap_err(),
            plan_delete(Some(note("a")), "a", Some(7)).unwrap_err(),
        ] {
            assert_eq!(failure.code, ErrorCode::Conflict);
            assert_eq!(failure.details["actualRevision"], 1);
        }
    }

    #[test]
    fn changing_a_missing_note_is_not_found() {
        assert_eq!(plan_delete(None, "a", None).unwrap_err().code, ErrorCode::NotFound);
        let failure = plan_update(None, "a", Some("x".into()), None, None, "now").unwrap_err();
        assert_eq!(failure.code, ErrorCode::NotFound);
    }

    #[test]
    fn titles_are_one_nonempty_line() {
        assert!(validate_title("Groceries").is_ok());
        for bad in ["", "  ", "two\nlines", &"x".repeat(TITLE_MAX_LENGTH + 1)] {
            assert_eq!(validate_title(bad).unwrap_err().code, ErrorCode::InvalidArgument, "{bad}");
        }
        assert!(validate_body(&"x".repeat(BODY_MAX_BYTES + 1)).is_err());
    }

    #[test]
    fn timestamps_are_utc_with_milliseconds() {
        let now = now();
        assert_eq!(now.len(), "2026-01-01T00:00:00.000Z".len(), "{now}");
        assert!(now.ends_with('Z'));
    }

    #[test]
    fn applied_changes_read_back_through_the_table_declaration() {
        let mut connection = store();
        apply(&mut connection, &Change::Create(note("groceries"))).unwrap();
        assert_eq!(find(&mut connection, "groceries").unwrap(), Some(note("groceries")));
        let change = plan_update(
            find(&mut connection, "groceries").unwrap(),
            "groceries",
            Some("New".into()),
            None,
            Some(1),
            "later",
        )
        .unwrap();
        apply(&mut connection, &change).unwrap();
        let stored = find(&mut connection, "groceries").unwrap().unwrap();
        assert_eq!((stored.title.as_str(), stored.revision), ("New", 2));
        apply(&mut connection, &Change::Delete(stored)).unwrap();
        assert_eq!(count(&mut connection).unwrap(), 0);
    }

    #[test]
    fn the_migrated_columns_are_the_declared_ones() {
        let mut connection = store();
        #[derive(QueryableByName)]
        struct Column {
            #[diesel(sql_type = diesel::sql_types::Text)]
            name: String,
        }
        let columns: Vec<String> = diesel::sql_query("SELECT name FROM pragma_table_info('notes')")
            .load::<Column>(&mut connection)
            .unwrap()
            .into_iter()
            .map(|column| column.name)
            .collect();
        assert_eq!(columns, ["name", "title", "body", "revision", "created_at", "updated_at"]);
    }

    #[test]
    fn pages_follow_name_order_and_say_when_more_follow() {
        let mut connection = store();
        for name in ["c", "a", "b"] {
            apply(&mut connection, &Change::Create(note(name))).unwrap();
        }
        let (first, more) = page(&mut connection, None, 2).unwrap();
        assert_eq!(first.iter().map(|note| note.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert!(more);
        let (rest, more) = page(&mut connection, Some("b"), 2).unwrap();
        assert_eq!(rest.iter().map(|note| note.name.as_str()).collect::<Vec<_>>(), ["c"]);
        assert!(!more);
    }
}
