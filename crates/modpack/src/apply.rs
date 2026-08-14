//! Applying one mod update file to the working databases.
//!
//! `.sql` files run as plain SQLite batches against the gameplay database — VP's SQL is
//! written for the game's own SQLite and nothing else. `.xml` files are the game's GameData
//! format: a `<GameData>` root whose children either declare a table (`<Table name="…">`)
//! or carry operations for one (`<Row>`, `<Replace>`, `<InsertOrIgnore>`, `<Update>`,
//! `<Delete>`). `Language_*` tables live in the localization database and every other table
//! in the gameplay database — routed per top-level element, exactly as the game routes them.
//!
//! Strictness is calibrated to what the game itself tolerates. A `<Row>` colliding with an
//! existing primary key or unique value is logged and skipped, because activating the same
//! mods in-game survives that too. An `<Update>` or `<Delete>` matching zero rows is silently
//! fine. Everything else — a missing table, a malformed file, an unknown construct — is a mod
//! bug and aborts the merge before anything is dumped.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use civ5vp_core::{BoundaryError, ProgressReporter, Stage};
use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

use crate::gamedata::{self, Element};

/// Apply one update file, routing per its content, reporting one progress line per file.
pub(crate) fn apply_update(
    path: &Path,
    gameplay: &mut Connection,
    text: &mut Connection,
    progress: &ProgressReporter,
) -> Result<(), BoundaryError> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());

    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);

    match extension.as_deref() {
        Some("sql") => {
            progress.report(Stage::Build, format!("Applying {name}."));
            let sql = fs::read_to_string(path)
                .map_err(|error| file_error(&name, format!("read: {error}")))?;
            gameplay
                .execute_batch(&sql)
                .map_err(|error| file_error(&name, error.to_string()))
        }
        Some("xml") => {
            progress.report(Stage::Build, format!("Applying {name}."));
            let bytes =
                fs::read(path).map_err(|error| file_error(&name, format!("read: {error}")))?;
            let document = gamedata::parse(&bytes).map_err(|detail| file_error(&name, detail))?;

            // One transaction per database per file: the merge either takes the whole file
            // or fails it, and a hundred thousand single-row autocommits would crawl.
            let gameplay_tx = gameplay
                .transaction()
                .map_err(|error| file_error(&name, error.to_string()))?;
            let text_tx = text
                .transaction()
                .map_err(|error| file_error(&name, error.to_string()))?;
            apply_gamedata(&document, &gameplay_tx, &text_tx, &name, progress)
                .map_err(|detail| file_error(&name, detail))?;
            gameplay_tx
                .commit()
                .and_then(|()| text_tx.commit())
                .map_err(|error| file_error(&name, error.to_string()))
        }
        _ => {
            // Not a database update at all — .lua, .dds and friends are the Core's problem.
            progress.report(
                Stage::Build,
                format!("Skipping {name} — not a database update."),
            );
            Ok(())
        }
    }
}

/// Rule 10's two halves for a failed update: the message names the file, the detail carries
/// the SQL or XML error.
fn file_error(file_name: &str, detail: String) -> BoundaryError {
    BoundaryError::new(
        format!(
            "Building the Modpack failed while applying {file_name} — remove or update that mod and try again."
        ),
        detail,
    )
}

fn apply_gamedata(
    document: &Element,
    gameplay: &Connection,
    text: &Connection,
    file_name: &str,
    progress: &ProgressReporter,
) -> Result<(), String> {
    if document.name != "GameData" {
        return Err(format!(
            "the root element is <{}>; a database update must be <GameData>",
            document.name
        ));
    }

    let route = |table: &str| {
        if table.starts_with("Language_") {
            text
        } else {
            gameplay
        }
    };

    for child in &document.children {
        match child.name.as_str() {
            "Table" => {
                let table = child
                    .attribute("name")
                    .ok_or_else(|| "<Table> without a name attribute".to_string())?;
                valid_name(table)?;
                create_table(route(table), table, child)?;
            }
            "DeleteMissingReferences" => {
                // Not part of VP's data. If a mod ever ships one, fail loudly so the
                // construct gets implemented deliberately instead of half-guessed.
                return Err(
                    "<DeleteMissingReferences> is not supported by the Modpack merge".to_string(),
                );
            }
            table => {
                valid_name(table)?;
                // Text for a language this game does not have — mods ship translations
                // for every language, the merged localization database only holds the
                // installed ones, and the game drops the rest on the floor. So does the
                // merge, out loud once per element.
                if table.starts_with("Language_") && !has_table(text, table)? {
                    progress.report(
                        Stage::Build,
                        format!(
                            "Skipped {table} text in {file_name} — this game does not \
                             have that language."
                        ),
                    );
                    continue;
                }
                apply_operations(route(table), table, child, file_name, progress)?;
            }
        }
    }
    Ok(())
}

/// Does the database hold this table — or a view standing in for it, the way the merged
/// localization database exposes the active language?
fn has_table(conn: &Connection, table: &str) -> Result<bool, String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('table', 'view') \
             AND name = ?1 COLLATE NOCASE",
            [table],
            |row| row.get(0),
        )
        .map_err(|error| format!("looking for {table}: {error}"))?;
    Ok(count > 0)
}

/// Table and column names get spliced into SQL (they cannot be bound), so only the character
/// set the game's own tables use is allowed through.
fn valid_name(name: &str) -> Result<(), String> {
    let plain = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    if plain {
        Ok(())
    } else {
        Err(format!("invalid table or column name {name:?}"))
    }
}

/// `CREATE TABLE IF NOT EXISTS` from a `<Table name="…">` declaration.
fn create_table(conn: &Connection, table: &str, declaration: &Element) -> Result<(), String> {
    struct Column {
        name: String,
        declared_type: String,
        primary_key: bool,
        autoincrement: bool,
    }

    let truthy =
        |value: Option<&str>| value.is_some_and(|value| value.eq_ignore_ascii_case("true"));

    let mut columns = Vec::new();
    let mut definitions = Vec::new();
    for child in &declaration.children {
        if child.name != "Column" {
            return Err(format!(
                "unexpected <{}> inside <Table name=\"{table}\">",
                child.name
            ));
        }
        let name = child
            .attribute("name")
            .ok_or_else(|| format!("a column of table {table} has no name"))?;
        valid_name(name)?;
        let declared_type = child
            .attribute("type")
            .ok_or_else(|| format!("column {name} of table {table} has no type"))?;
        valid_type(declared_type)?;
        columns.push(Column {
            name: name.to_string(),
            declared_type: declared_type.to_string(),
            primary_key: truthy(child.attribute("primarykey")),
            autoincrement: truthy(child.attribute("autoincrement")),
        });
        let mut definition = format!("\"{name}\" {declared_type}");
        if truthy(child.attribute("unique")) {
            definition.push_str(" UNIQUE");
        }
        if truthy(child.attribute("notnull")) {
            definition.push_str(" NOT NULL");
        }
        if let Some(default) = child.attribute("default") {
            let is_boolean = declared_type.eq_ignore_ascii_case("boolean");
            definition.push_str(" DEFAULT ");
            definition.push_str(&default_literal(default, is_boolean));
        }
        // A reference="OtherTable(Col)" attribute exists in the wild; the game does not
        // enforce foreign keys on this path and neither do we.
        definitions.push(definition);
    }

    let primary_keys: Vec<&Column> = columns.iter().filter(|column| column.primary_key).collect();
    // AUTOINCREMENT is only meaningful — and only legal — inline on a single INTEGER
    // primary key. A composite key becomes a trailing clause and loses it.
    let inline_autoincrement = match primary_keys.as_slice() {
        [only] => only.autoincrement && only.declared_type.eq_ignore_ascii_case("integer"),
        _ => false,
    };
    if inline_autoincrement {
        for (column, definition) in columns.iter().zip(definitions.iter_mut()) {
            if column.primary_key {
                let clause_at = format!("\"{}\" {}", column.name, column.declared_type).len();
                definition.insert_str(clause_at, " PRIMARY KEY AUTOINCREMENT");
            }
        }
    } else if !primary_keys.is_empty() {
        let names: Vec<String> = primary_keys
            .iter()
            .map(|column| format!("\"{}\"", column.name))
            .collect();
        definitions.push(format!("PRIMARY KEY ({})", names.join(", ")));
    }

    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS \"{table}\" ({})",
        definitions.join(", ")
    ))
    .map_err(|error| format!("creating table {table}: {error}"))
}

/// The declared type is spliced into the CREATE, so it gets the same guard as names — plus
/// `(`, `)` and space, for the `varchar(64)`-shaped types older mods carry.
fn valid_type(declared_type: &str) -> Result<(), String> {
    let plain = !declared_type.is_empty()
        && declared_type
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'(' | b')' | b' '));
    if plain {
        Ok(())
    } else {
        Err(format!("invalid column type {declared_type:?}"))
    }
}

/// Quote a `default="…"` attribute for the CREATE: booleans become 0/1, numbers pass
/// through bare, everything else is single-quoted with `''` escaping.
fn default_literal(raw: &str, is_boolean: bool) -> String {
    if is_boolean {
        if raw.eq_ignore_ascii_case("true") {
            return "1".to_string();
        }
        if raw.eq_ignore_ascii_case("false") {
            return "0".to_string();
        }
    }
    if !raw.is_empty() && raw.parse::<f64>().is_ok() {
        return raw.to_string();
    }
    format!("'{}'", raw.replace('\'', "''"))
}

fn apply_operations(
    conn: &Connection,
    table: &str,
    container: &Element,
    file_name: &str,
    progress: &ProgressReporter,
) -> Result<(), String> {
    // Which columns are declared boolean decides the true/false → 1/0 conversion. On a
    // missing table this set is just empty; the operation itself then fails with the real
    // "no such table" error.
    let booleans = boolean_columns(conn, table)?;

    for operation in &container.children {
        match operation.name.as_str() {
            "Row" => insert(
                conn,
                table,
                operation,
                &booleans,
                InsertKind::Plain,
                file_name,
                progress,
            )?,
            "Replace" => insert(
                conn,
                table,
                operation,
                &booleans,
                InsertKind::Replace,
                file_name,
                progress,
            )?,
            "InsertOrIgnore" => insert(
                conn,
                table,
                operation,
                &booleans,
                InsertKind::Ignore,
                file_name,
                progress,
            )?,
            "Update" => update(conn, table, operation, &booleans)?,
            "Delete" => delete(conn, table, operation, &booleans)?,
            "DeleteMissingReferences" => {
                return Err(format!(
                    "<DeleteMissingReferences> on table {table} is not supported by the Modpack merge"
                ));
            }
            other => {
                return Err(format!("unknown operation <{other}> on table {table}"));
            }
        }
    }
    Ok(())
}

fn boolean_columns(conn: &Connection, table: &str) -> Result<HashSet<String>, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info(\"{table}\")"))
        .map_err(|error| format!("reading the schema of {table}: {error}"))?;
    let mut booleans = HashSet::new();
    let mut rows = stmt
        .query([])
        .map_err(|error| format!("reading the schema of {table}: {error}"))?;
    while let Some(row) = rows
        .next()
        .map_err(|error| format!("reading the schema of {table}: {error}"))?
    {
        let name: String = row
            .get(1)
            .map_err(|error| format!("reading the schema of {table}: {error}"))?;
        let declared_type: String = row
            .get(2)
            .map_err(|error| format!("reading the schema of {table}: {error}"))?;
        if declared_type.eq_ignore_ascii_case("boolean") {
            booleans.insert(name);
        }
    }
    Ok(booleans)
}

/// Column/value pairs of one operation element: attributes first, then child elements.
/// `<Row Tag="X"><Cost>5</Cost></Row>` yields `[("Tag", "X"), ("Cost", "5")]`.
fn pairs_of(
    element: &Element,
    booleans: &HashSet<String>,
    table: &str,
) -> Result<Vec<(String, Value)>, String> {
    let mut pairs = Vec::new();
    for (column, value) in &element.attributes {
        valid_name(column)?;
        pairs.push((column.clone(), convert(column, value, booleans)));
    }
    for child in &element.children {
        if !child.children.is_empty() {
            return Err(format!(
                "<{}> inside <{}> of table {table} has nested elements",
                child.name, element.name
            ));
        }
        valid_name(&child.name)?;
        pairs.push((
            child.name.clone(),
            convert(&child.name, &child.text, booleans),
        ));
    }
    Ok(pairs)
}

/// true/false on a boolean-declared column becomes 1/0; everything else is bound as the
/// text it came as — SQLite's column affinity does the rest, exactly as it does in-game.
fn convert(column: &str, value: &str, booleans: &HashSet<String>) -> Value {
    if booleans.contains(column) {
        if value.eq_ignore_ascii_case("true") {
            return Value::Integer(1);
        }
        if value.eq_ignore_ascii_case("false") {
            return Value::Integer(0);
        }
    }
    Value::Text(value.to_string())
}

#[derive(Clone, Copy, PartialEq)]
enum InsertKind {
    Plain,
    Replace,
    Ignore,
}

fn insert(
    conn: &Connection,
    table: &str,
    operation: &Element,
    booleans: &HashSet<String>,
    kind: InsertKind,
    file_name: &str,
    progress: &ProgressReporter,
) -> Result<(), String> {
    let pairs = pairs_of(operation, booleans, table)?;
    let verb = match kind {
        InsertKind::Plain => "INSERT",
        InsertKind::Replace => "INSERT OR REPLACE",
        InsertKind::Ignore => "INSERT OR IGNORE",
    };
    let sql = if pairs.is_empty() {
        format!("{verb} INTO \"{table}\" DEFAULT VALUES")
    } else {
        let columns: Vec<String> = pairs
            .iter()
            .map(|(column, _)| format!("\"{column}\""))
            .collect();
        let placeholders: Vec<String> = (1..=pairs.len()).map(|n| format!("?{n}")).collect();
        format!(
            "{verb} INTO \"{table}\" ({}) VALUES ({})",
            columns.join(", "),
            placeholders.join(", ")
        )
    };
    let result = conn.execute(&sql, params_from_iter(pairs.iter().map(|(_, value)| value)));
    match result {
        Ok(_) => Ok(()),
        // The game does not abort a whole activation over a duplicate row and neither do we:
        // log it, skip it, keep the first one — the same outcome an in-game activation has.
        Err(error) if kind == InsertKind::Plain && is_duplicate(&error) => {
            progress.report(
                Stage::Build,
                format!("Skipping a duplicate {table} row in {file_name}."),
            );
            Ok(())
        }
        Err(error) => Err(format!("inserting into {table}: {error}")),
    }
}

/// A primary-key or unique-constraint collision — the one insert failure that is tolerated.
fn is_duplicate(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(inner, _) if matches!(
            inner.extended_code,
            rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY | rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
        )
    )
}

fn update(
    conn: &Connection,
    table: &str,
    operation: &Element,
    booleans: &HashSet<String>,
) -> Result<(), String> {
    let mut sets = Vec::new();
    let mut conditions = Vec::new();
    for child in &operation.children {
        match child.name.as_str() {
            "Set" => sets.extend(pairs_of(child, booleans, table)?),
            "Where" => conditions.extend(pairs_of(child, booleans, table)?),
            other => {
                return Err(format!(
                    "unexpected <{other}> inside <Update> of table {table}"
                ));
            }
        }
    }
    if sets.is_empty() {
        return Err(format!("<Update> on table {table} has no <Set>"));
    }

    let assignments: Vec<String> = sets
        .iter()
        .enumerate()
        .map(|(index, (column, _))| format!("\"{column}\" = ?{}", index + 1))
        .collect();
    let mut sql = format!("UPDATE \"{table}\" SET {}", assignments.join(", "));
    if !conditions.is_empty() {
        let tests: Vec<String> = conditions
            .iter()
            .enumerate()
            .map(|(index, (column, _))| format!("\"{column}\" = ?{}", sets.len() + index + 1))
            .collect();
        sql.push_str(" WHERE ");
        sql.push_str(&tests.join(" AND "));
    }

    let values = sets.iter().chain(conditions.iter()).map(|(_, value)| value);
    // Zero rows matched is fine — the game tolerates updates aimed at content that is not
    // installed. A missing table is not: execute fails and the merge stops.
    conn.execute(&sql, params_from_iter(values))
        .map(|_| ())
        .map_err(|error| format!("updating {table}: {error}"))
}

fn delete(
    conn: &Connection,
    table: &str,
    operation: &Element,
    booleans: &HashSet<String>,
) -> Result<(), String> {
    let conditions = pairs_of(operation, booleans, table)?;
    let sql = if conditions.is_empty() {
        format!("DELETE FROM \"{table}\"")
    } else {
        let tests: Vec<String> = conditions
            .iter()
            .enumerate()
            .map(|(index, (column, _))| format!("\"{column}\" = ?{}", index + 1))
            .collect();
        format!("DELETE FROM \"{table}\" WHERE {}", tests.join(" AND "))
    };
    conn.execute(
        &sql,
        params_from_iter(conditions.iter().map(|(_, value)| value)),
    )
    .map(|_| ())
    .map_err(|error| format!("deleting from {table}: {error}"))
}
