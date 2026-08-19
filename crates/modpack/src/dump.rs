//! The two Override dumps, byte for byte as `ModpackMaker.lua` writes them.
//!
//! The format is defined by that Lua (the in-game "Modpack Maker for VP"), not by us: the
//! game demonstrably loads its output, so this module reproduces it exactly rather than
//! emitting anything nicer. Its one structural habit matters everywhere: every
//! `Game.WriteMPMP(chunk)` call appends a newline after the chunk. [`MakerFile::chunk`]
//! models that call, so a chunk that itself ends in `\n` - the `<Table>` block, the
//! `<X><Delete/>` opener, the `</X>` closer, the gameplay `</GameData>\n` - is followed by
//! a blank line in the file, and a chunk that does not - `</Row>`, every localization line -
//! is not. Those blank lines are load-bearing for byte-compatibility; do not tidy them.
//!
//! Two escaping sets, also the Lua's: tags/attributes escape `& < > " '`, text between tags
//! escapes only `& < >`. Values are printed the way Lua's `tostring` prints them - integers
//! bare, REALs through `%.14g` - and a value that is NULL or an empty string is simply not
//! written (zeros are).
//!
//! One known divergence from the in-game bytes: the column `type` attribute is whatever
//! `PRAGMA table_info` reports, and the bundled SQLite interns standard type names at parse
//! time - a column declared `integer` comes back `INTEGER`, where the game's 2010 SQLite
//! preserves the declared case. The game reads type names case-insensitively, and `boolean`
//! is not a standard name so the Lua's boolean-default special case still fires; but a diff
//! against an in-game dump must compare `type=` case-insensitively.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use civ5vp_core::BoundaryError;
use rusqlite::Connection;
use rusqlite::types::ValueRef;

/// Both dumps open with this; `Game.WriteMPMP` supplies the newline after `<GameData>`.
const HEADER: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
                      <!-- This modpack is only compatible with the Community Patch DLL -->\n\
                      <GameData>";

/// Tables the game engine inserts into itself - the Lua dumps neither their schema nor
/// their rows, and neither do we.
const SKIPPED_TABLES: [&str; 7] = [
    "ApplicationInfo",
    "DownloadableContent",
    "MapScriptOptionPossibleValues",
    "MapScriptOptions",
    "MapScriptRequiredDLC",
    "MapScripts",
    "ScannedFiles",
];

/// Tables that exist in-game before any XML runs - their rows are dumped but their
/// `<Table>` declaration is not. The exact list from the Lua.
const SKIPPED_SCHEMAS: [&str; 21] = [
    "ArtDefine_LandmarkTypes",
    "ArtDefine_Landmarks",
    "ArtDefine_StrategicView",
    "ArtDefine_UnitInfoMemberInfos",
    "ArtDefine_UnitInfos",
    "ArtDefine_UnitMemberCombatWeapons",
    "ArtDefine_UnitMemberCombats",
    "ArtDefine_UnitMemberInfos",
    "Audio_2DSounds",
    "Audio_3DSounds",
    "Audio_ScriptTypes",
    "Audio_SoundLoadTypes",
    "Audio_SoundScapeElementScripts",
    "Audio_SoundScapeElements",
    "Audio_SoundScapes",
    "Audio_SoundTypes",
    "Audio_Sounds",
    "Audio_SpeakerChannels",
    "Map_Folders",
    "Map_Sizes",
    "Maps",
];

/// The Lua's `LANGUAGE_LIST`, in its exact order. Every language gets its element written,
/// with data or without.
const LANGUAGES: [&str; 10] = [
    "en_US",
    "DE_DE",
    "ES_ES",
    "FR_FR",
    "IT_IT",
    "JA_JP",
    "KO_KR",
    "PL_PL",
    "RU_RU",
    "ZH_HANT_HK",
];

/// One dump file, written the way the game writes one: [`MakerFile::chunk`] is one
/// `Game.WriteMPMP` call - the chunk, then the newline the game appends.
struct MakerFile<W: Write> {
    out: W,
}

impl<W: Write> MakerFile<W> {
    fn chunk(&mut self, chunk: &str) -> std::io::Result<()> {
        self.out.write_all(chunk.as_bytes())?;
        self.out.write_all(b"\n")
    }
}

fn dump_error(detail: String) -> BoundaryError {
    BoundaryError::new(
        "Writing the Modpack's database dump failed - check free disk space and try again.",
        detail,
    )
}

fn create_dump(path: &Path) -> Result<MakerFile<BufWriter<File>>, BoundaryError> {
    let file = File::create(path)
        .map_err(|error| dump_error(format!("create {}: {error}", path.display())))?;
    Ok(MakerFile {
        out: BufWriter::new(file),
    })
}

/// Everything the merged gameplay database holds, as the game's GameData XML.
pub(crate) fn dump_gameplay(conn: &Connection, path: &Path) -> Result<(), BoundaryError> {
    let mut dump = create_dump(path)?;
    let io = |error: std::io::Error| dump_error(format!("write {}: {error}", path.display()));

    dump.chunk(HEADER).map_err(io)?;

    let tables = table_names(conn).map_err(dump_error)?;
    for table in &tables {
        if SKIPPED_TABLES.contains(&table.as_str()) {
            continue;
        }
        let columns = columns_of(conn, table).map_err(dump_error)?;

        if !SKIPPED_SCHEMAS.contains(&table.as_str()) {
            dump.chunk(&table_structure(table, &columns)).map_err(io)?;
        }

        dump.chunk(&format!("\t<{table}>\n\t\t<Delete/>\n"))
            .map_err(io)?;
        dump_rows(conn, table, &mut dump, path)?;
        dump.chunk(&format!("\t</{table}>\n")).map_err(io)?;
    }

    dump.chunk("</GameData>\n").map_err(io)?;
    dump.out.flush().map_err(io)
}

fn table_names(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT name FROM sqlite_master \
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .map_err(|error| format!("listing tables: {error}"))?;
    let names = stmt
        .query_map([], |row| row.get(0))
        .and_then(Iterator::collect)
        .map_err(|error| format!("listing tables: {error}"))?;
    Ok(names)
}

struct ColumnInfo {
    name: String,
    declared_type: String,
    notnull: i64,
    default: Option<String>,
    primary_key: i64,
}

fn columns_of(conn: &Connection, table: &str) -> Result<Vec<ColumnInfo>, String> {
    let context = |error: rusqlite::Error| format!("reading the schema of {table}: {error}");
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info(\"{table}\")"))
        .map_err(context)?;
    let mut columns = Vec::new();
    let mut rows = stmt.query([]).map_err(context)?;
    while let Some(row) = rows.next().map_err(context)? {
        let default = match render_value(row.get_ref(4).map_err(context)?) {
            Ok(value) => value,
            Err(detail) => return Err(format!("default of a column of {table}: {detail}")),
        };
        columns.push(ColumnInfo {
            name: row.get(1).map_err(context)?,
            declared_type: row.get(2).map_err(context)?,
            notnull: row.get(3).map_err(context)?,
            default,
            primary_key: row.get(5).map_err(context)?,
        });
    }
    Ok(columns)
}

/// The `<Table name="…">` block, attribute for attribute in the Lua's order. The
/// `autoincrement` and `unique` attributes are emitted from the column's *name* - `ID` and
/// `Type` - combined with the primary-key flag, because that is all the Lua looks at.
fn table_structure(table: &str, columns: &[ColumnInfo]) -> String {
    let mut block = format!("\t<Table name=\"{table}\">\n");
    for column in columns {
        block.push_str(&format!(
            "\t\t<Column name=\"{}\" type=\"{}\"",
            column.name,
            escape_tag(&column.declared_type)
        ));
        if column.primary_key > 0 {
            block.push_str(" primarykey=\"true\"");
        }
        if column.name == "ID" && column.primary_key > 0 {
            block.push_str(" autoincrement=\"true\"");
        }
        if column.name == "Type" && column.primary_key > 0 {
            block.push_str(" unique=\"true\"");
        }
        if column.notnull > 0 {
            block.push_str(" notnull=\"true\"");
        }
        // The Lua strips every single quote out of the default expression, maps a boolean
        // 0/1 back to false/true, and drops empty and NULL defaults.
        let mut default = column.default.clone().unwrap_or_default().replace('\'', "");
        if column.declared_type == "boolean" {
            if default == "0" {
                default = "false".to_string();
            } else if default == "1" {
                default = "true".to_string();
            }
        }
        if !default.is_empty() && default != "NULL" {
            block.push_str(&format!(" default=\"{}\"", escape_tag(&default)));
        }
        block.push_str("/>\n");
    }
    block.push_str("\t</Table>\n");
    block
}

fn dump_rows<W: Write>(
    conn: &Connection,
    table: &str,
    dump: &mut MakerFile<W>,
    path: &Path,
) -> Result<(), BoundaryError> {
    let io = |error: std::io::Error| dump_error(format!("write {}: {error}", path.display()));
    let context = |error: rusqlite::Error| dump_error(format!("reading {table}: {error}"));

    let mut stmt = conn
        .prepare(&format!("SELECT * FROM \"{table}\""))
        .map_err(context)?;
    let names: Vec<String> = stmt
        .column_names()
        .into_iter()
        .map(str::to_string)
        .collect();
    let mut rows = stmt.query([]).map_err(context)?;
    while let Some(row) = rows.next().map_err(context)? {
        let mut chunk = String::from("\t\t<Row>\n");
        for (index, column) in names.iter().enumerate() {
            let value = render_value(row.get_ref(index).map_err(context)?)
                .map_err(|detail| dump_error(format!("{table}.{column}: {detail}")))?;
            // NULL and the empty string are not written; a stored 0 is.
            if let Some(value) = value
                && !value.is_empty()
            {
                chunk.push_str(&format!(
                    "\t\t\t<{column}>{}</{column}>\n",
                    escape_text(&value)
                ));
            }
        }
        chunk.push_str("\t\t</Row>");
        dump.chunk(&chunk).map_err(io)?;
    }
    Ok(())
}

/// The merged localization database, every language of the Lua's list in its order.
pub(crate) fn dump_text(conn: &Connection, path: &Path) -> Result<(), BoundaryError> {
    let mut dump = create_dump(path)?;
    let io = |error: std::io::Error| dump_error(format!("write {}: {error}", path.display()));

    dump.chunk(HEADER).map_err(io)?;

    for language in LANGUAGES {
        let table = format!("Language_{language}");
        dump.chunk(&format!("\t<{table}>")).map_err(io)?;
        // A language that was never loaded - table missing or empty - still gets its open
        // and close tags; the Lua only warns about it.
        if table_exists(conn, &table).map_err(dump_error)? {
            dump_language(conn, &table, &mut dump, path)?;
        }
        dump.chunk(&format!("\t</{table}>")).map_err(io)?;
    }

    dump.chunk("</GameData>").map_err(io)?;
    dump.out.flush().map_err(io)
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, String> {
    // Views count: in the game's merged localization database the active language is a
    // *view* over `LocalizedText` (with INSTEAD OF triggers carrying writes into it), and
    // the in-game Modpack Maker dumps straight through it. Only the never-loaded
    // languages are plain - and empty - tables.
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('table', 'view') AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .map_err(|error| format!("looking for {table}: {error}"))?;
    Ok(count > 0)
}

fn dump_language<W: Write>(
    conn: &Connection,
    table: &str,
    dump: &mut MakerFile<W>,
    path: &Path,
) -> Result<(), BoundaryError> {
    let io = |error: std::io::Error| dump_error(format!("write {}: {error}", path.display()));
    let context = |error: rusqlite::Error| dump_error(format!("reading {table}: {error}"));

    let mut stmt = conn
        .prepare(&format!("SELECT * FROM \"{table}\""))
        .map_err(context)?;
    let names: Vec<String> = stmt
        .column_names()
        .into_iter()
        .map(str::to_string)
        .collect();
    let position = |name: &str| names.iter().position(|column| column == name);
    let tag_at = position("Tag");
    let parts = [
        (position("Text"), "Text"),
        (position("Gender"), "Gender"),
        (position("Plurality"), "Plurality"),
    ];

    let mut rows = stmt.query([]).map_err(context)?;
    while let Some(row) = rows.next().map_err(context)? {
        let fetch = |index: Option<usize>| -> Result<Option<String>, BoundaryError> {
            match index {
                Some(index) => render_value(row.get_ref(index).map_err(context)?)
                    .map_err(|detail| dump_error(format!("{table}: {detail}"))),
                None => Ok(None),
            }
        };
        let tag = fetch(tag_at)?.unwrap_or_default();
        dump.chunk(&format!("\t\t<Replace Tag=\"{}\">", escape_tag(&tag)))
            .map_err(io)?;
        for (index, element) in parts {
            // Unlike the gameplay dump, an empty string is written here - the Lua only
            // checks for nil, and an empty <Text> survives the round trip.
            if let Some(value) = fetch(index)? {
                dump.chunk(&format!("\t\t\t<{element}>")).map_err(io)?;
                dump.chunk(&format!("\t\t\t\t{}", escape_text(&value)))
                    .map_err(io)?;
                dump.chunk(&format!("\t\t\t</{element}>")).map_err(io)?;
            }
        }
        dump.chunk("\t\t</Replace>").map_err(io)?;
    }
    Ok(())
}

/// A stored value as Lua's `tostring` would print it. `None` is NULL; BLOBs are an error -
/// nothing the game dumps has one, so meeting one means the merge produced something the
/// format cannot carry.
fn render_value(value: ValueRef<'_>) -> Result<Option<String>, String> {
    match value {
        ValueRef::Null => Ok(None),
        ValueRef::Integer(value) => Ok(Some(value.to_string())),
        ValueRef::Real(value) => Ok(Some(lua_number(value))),
        ValueRef::Text(bytes) => std::str::from_utf8(bytes)
            .map(|text| Some(text.to_string()))
            .map_err(|error| format!("text value is not valid UTF-8: {error}")),
        ValueRef::Blob(_) => Err("a BLOB value cannot be dumped as GameData XML".to_string()),
    }
}

/// C's `%.14g`, which is what Lua 5.1's `tostring` uses for numbers: at most 14 significant
/// digits, trailing zeros dropped, scientific notation outside 1e-4..1e14.
fn lua_number(value: f64) -> String {
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.to_string();
    }

    // Round to 14 significant digits first; the exponent of the *rounded* value picks the
    // notation, exactly as printf does.
    let scientific = format!("{value:.13e}");
    let Some((mantissa, exponent)) = scientific.split_once('e') else {
        return scientific;
    };
    let Ok(exponent) = exponent.parse::<i32>() else {
        return scientific;
    };

    if !(-4..14).contains(&exponent) {
        let mantissa = trim_fraction(mantissa);
        let sign = if exponent < 0 { '-' } else { '+' };
        format!("{mantissa}e{sign}{:02}", exponent.abs())
    } else {
        let precision = usize::try_from(13 - exponent).unwrap_or(0);
        trim_fraction(&format!("{value:.precision$}"))
    }
}

/// Drop trailing zeros of a decimal fraction, and the point itself if nothing survives.
fn trim_fraction(number: &str) -> String {
    if !number.contains('.') {
        return number.to_string();
    }
    number
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

/// The Lua's `EscapeXmlTags`: everything going inside a tag or attribute.
fn escape_tag(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// The Lua's `EscapeXml`: text between tags escapes only the three that break parsing.
fn escape_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::lua_number;

    /// Lua 5.1's `tostring` is `%.14g`; these are the renderings the game's own dump shows.
    #[test]
    fn reals_print_the_way_lua_tostring_prints_them() {
        assert_eq!(lua_number(0.5), "0.5");
        assert_eq!(lua_number(1.0), "1");
        assert_eq!(lua_number(-2.25), "-2.25");
        assert_eq!(lua_number(0.0), "0");
        assert_eq!(lua_number(100.0), "100");
        assert_eq!(lua_number(0.1), "0.1");
        assert_eq!(lua_number(1.0 / 3.0), "0.33333333333333");
        assert_eq!(lua_number(1e-5), "1e-05");
        assert_eq!(lua_number(1e20), "1e+20");
        assert_eq!(lua_number(0.0001), "0.0001");
        assert_eq!(lua_number(123456789012345.0), "1.2345678901234e+14");
    }
}
