//! The `ModpackAssembler` boundary, exercised the way the Core will use it: build fixture
//! bases with rusqlite, hand `SqliteModpackAssembler` a job, assert on the dumps and the
//! progress lines. No game data, no network — this is the fast suite.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use civ5vp_core::{
    BoundaryError, CacheState, ModpackAssembler, ModpackDatabaseJob, ProgressEvent,
    ProgressReporter,
};
use civ5vp_modpack::SqliteModpackAssembler;
use rusqlite::Connection;

/// Create a fixture database. The trailing pragma forces the header to disk so even an
/// "empty" base is a real SQLite file.
fn create_db(path: &Path, sql: &str) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(sql).unwrap();
    conn.pragma_update(None, "user_version", 1).unwrap();
}

#[derive(Debug)]
struct Merged {
    gameplay: String,
    text: String,
    progress: Vec<String>,
}

fn run_merge(
    gameplay_sql: &str,
    text_sql: &str,
    updates: &[(&str, &str)],
) -> Result<Merged, BoundaryError> {
    let dir = tempfile::tempdir().unwrap();
    let job = job_in(dir.path(), updates);
    create_db(&job.gameplay_base, gameplay_sql);
    create_db(&job.text_base, text_sql);
    for ((name, content), path) in updates.iter().zip(job.updates.iter()) {
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), *name);
        fs::write(path, content).unwrap();
    }

    let (sender, receiver) = mpsc::channel::<ProgressEvent>();
    SqliteModpackAssembler::new().merge_and_dump(&job, &ProgressReporter::to_channel(sender))?;

    Ok(Merged {
        gameplay: fs::read_to_string(&job.gameplay_dump).unwrap(),
        text: fs::read_to_string(&job.text_dump).unwrap(),
        progress: receiver.try_iter().map(|event| event.message).collect(),
    })
}

fn job_in(dir: &Path, updates: &[(&str, &str)]) -> ModpackDatabaseJob {
    ModpackDatabaseJob {
        gameplay_base: dir.join("gameplay-base.db"),
        text_base: dir.join("text-base.db"),
        updates: updates
            .iter()
            .map(|(name, _)| dir.join(name))
            .collect::<Vec<PathBuf>>(),
        gameplay_dump: dir.join("gameplay-dump.xml"),
        text_dump: dir.join("text-dump.xml"),
        scratch_dir: dir.join("scratch"),
    }
}

// ---------------------------------------------------------------- cache_state

#[test]
fn a_pristine_cache_offers_itself_as_the_modpack_base() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("Civ5DebugDatabase.db");
    create_db(
        &db,
        "CREATE TABLE Civilizations (Type text); INSERT INTO Civilizations VALUES ('CIVILIZATION_ROME');",
    );

    let state = SqliteModpackAssembler::new().cache_state(&db).unwrap();

    assert_eq!(state, CacheState::Pristine);
}

#[test]
fn a_cache_from_a_modded_session_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("Civ5DebugDatabase.db");
    create_db(
        &db,
        "CREATE TABLE Civilizations (Type text); \
         INSERT INTO Civilizations VALUES ('CIVILIZATION_ROME'); \
         CREATE TABLE CustomModOptions (Name text);",
    );

    let state = SqliteModpackAssembler::new().cache_state(&db).unwrap();

    assert_eq!(state, CacheState::Modded);
}

#[test]
fn an_unreadable_cache_tells_the_user_to_launch_the_game() {
    let dir = tempfile::tempdir().unwrap();
    let not_a_db = dir.path().join("Civ5DebugDatabase.db");
    fs::write(&not_a_db, "this is not a database").unwrap();

    let error = SqliteModpackAssembler::new()
        .cache_state(&not_a_db)
        .unwrap_err();

    assert!(
        error.message().contains("start Civilization V"),
        "the message must tell the user the fix: {}",
        error.message()
    );
}

#[test]
fn an_empty_civilizations_table_is_not_a_usable_base() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("Civ5DebugDatabase.db");
    create_db(&db, "CREATE TABLE Civilizations (Type text);");

    let error = SqliteModpackAssembler::new().cache_state(&db).unwrap_err();

    assert!(error.message().contains("start Civilization V"));
}

// ---------------------------------------------------------------- applying updates

#[test]
fn sql_updates_run_against_the_gameplay_database() {
    let merged = run_merge(
        "CREATE TABLE Beliefs (Type text, Cost integer);",
        "",
        &[(
            "changes.sql",
            "INSERT INTO Beliefs (Type, Cost) VALUES ('BELIEF_X', 1);\n\
             UPDATE Beliefs SET Cost = 5 WHERE Type = 'BELIEF_X';",
        )],
    )
    .unwrap();

    assert!(merged.gameplay.contains("<Type>BELIEF_X</Type>"));
    assert!(merged.gameplay.contains("<Cost>5</Cost>"));
    assert!(
        merged
            .progress
            .contains(&"Applying changes.sql.".to_string())
    );
}

/// The Community Patch's FixTypeConstraints.sql rebuilds tables via CREATE `_FIX` /
/// DROP / ALTER RENAME, and the rebuilt tables carry `REFERENCES Language_en_US` clauses
/// that dangle by design — text lives in the other database and the game never enforces
/// foreign keys. Modern SQLite re-validates the whole schema on a rename and would abort;
/// the merge must run with the game's legacy rename semantics instead.
#[test]
fn a_rebuild_with_dangling_language_references_merges_like_the_games_own_sqlite() {
    let merged = run_merge(
        "CREATE TABLE Calendars (Type text, Description text);
         INSERT INTO Calendars VALUES ('CALENDAR_DEFAULT', 'TXT_KEY_DEFAULT');",
        "",
        &[(
            "FixTypeConstraints.sql",
            "CREATE TABLE Calendars_FIX (
                 Type text NOT NULL UNIQUE,
                 Description text REFERENCES Language_en_US (Tag)
             );
             INSERT INTO Calendars_FIX SELECT * FROM Calendars;
             DROP TABLE Calendars;
             ALTER TABLE Calendars_FIX RENAME TO Calendars;",
        )],
    )
    .expect("the game's own SQL must merge");

    assert!(merged.gameplay.contains("<Type>CALENDAR_DEFAULT</Type>"));
    assert!(
        merged
            .gameplay
            .contains("<Description>TXT_KEY_DEFAULT</Description>")
    );
}

/// Mods ship translations for every language; the merged localization database only holds
/// the installed ones (InGame Editor's `IGE_ZH_CN.xml` was the first real hit). The game
/// drops text for absent languages, so the merge does the same, out loud.
#[test]
fn text_for_a_language_this_game_does_not_have_is_skipped() {
    let merged = run_merge(
        "",
        "CREATE TABLE Language_en_US (Tag text, Text text);",
        &[(
            "IGE_ZH_CN.xml",
            "<GameData>\n\
             \t<Language_zh_CN>\n\
             \t\t<Row Tag=\"TXT_KEY_IGE\"><Text>\u{7f16}\u{8f91}\u{5668}</Text></Row>\n\
             \t</Language_zh_CN>\n\
             \t<Language_en_US>\n\
             \t\t<Row Tag=\"TXT_KEY_IGE\"><Text>Editor</Text></Row>\n\
             \t</Language_en_US>\n\
             </GameData>",
        )],
    )
    .expect("absent-language text must not stop the merge");

    assert!(
        merged.text.contains("<Replace Tag=\"TXT_KEY_IGE\">"),
        "the installed language's text still lands"
    );
    assert!(
        merged
            .progress
            .iter()
            .any(|line| line.contains("Language_zh_CN") && line.contains("IGE_ZH_CN.xml")),
        "the skip is said out loud, got: {:?}",
        merged.progress
    );
}

#[test]
fn a_table_element_creates_the_table_with_its_constraints() {
    let merged = run_merge(
        "",
        "",
        &[(
            "schema.xml",
            "<GameData>\n\
             \t<Table name=\"MyPairs\">\n\
             \t\t<Column name=\"A\" type=\"text\" primarykey=\"true\"/>\n\
             \t\t<Column name=\"B\" type=\"integer\" primarykey=\"true\"/>\n\
             \t</Table>\n\
             \t<Table name=\"MyFlags\">\n\
             \t\t<Column name=\"ID\" type=\"integer\" primarykey=\"true\" autoincrement=\"true\"/>\n\
             \t\t<Column name=\"Enabled\" type=\"boolean\" default=\"true\"/>\n\
             \t</Table>\n\
             </GameData>",
        )],
    )
    .unwrap();

    // The composite primary key marks both columns; autoincrement and the boolean default
    // survive the round trip through the created table's real schema. Standard type names
    // come back uppercase: the bundled SQLite interns them at parse time (the game's 2010
    // SQLite preserves the declared case; the game reads both).
    assert!(
        merged
            .gameplay
            .contains("<Column name=\"A\" type=\"TEXT\" primarykey=\"true\"/>")
    );
    assert!(
        merged
            .gameplay
            .contains("<Column name=\"B\" type=\"INTEGER\" primarykey=\"true\"/>")
    );
    assert!(merged.gameplay.contains(
        "<Column name=\"ID\" type=\"INTEGER\" primarykey=\"true\" autoincrement=\"true\"/>"
    ));
    assert!(
        merged
            .gameplay
            .contains("<Column name=\"Enabled\" type=\"boolean\" default=\"true\"/>")
    );
}

#[test]
fn row_values_come_from_attributes_and_elements_alike() {
    let merged = run_merge(
        "CREATE TABLE Traits (Type text, IsFree boolean, Level integer);",
        "",
        &[(
            "rows.xml",
            "<GameData>\n\
             \t<Traits>\n\
             \t\t<Row Type=\"TRAIT_A\">\n\
             \t\t\t<IsFree>true</IsFree>\n\
             \t\t\t<Level>3</Level>\n\
             \t\t</Row>\n\
             \t</Traits>\n\
             </GameData>",
        )],
    )
    .unwrap();

    assert!(merged.gameplay.contains("<Type>TRAIT_A</Type>"));
    // true on a boolean-declared column is stored — and therefore dumped — as 1.
    assert!(merged.gameplay.contains("<IsFree>1</IsFree>"));
    assert!(merged.gameplay.contains("<Level>3</Level>"));
}

#[test]
fn replace_overwrites_and_insert_or_ignore_defers() {
    let merged = run_merge(
        "CREATE TABLE Defines (Name text PRIMARY KEY, Value integer); \
         INSERT INTO Defines VALUES ('X', 1);",
        "",
        &[(
            "defines.xml",
            "<GameData>\n\
             \t<Defines>\n\
             \t\t<Replace Name=\"X\" Value=\"2\"/>\n\
             \t\t<InsertOrIgnore Name=\"X\" Value=\"9\"/>\n\
             \t</Defines>\n\
             </GameData>",
        )],
    )
    .unwrap();

    assert!(merged.gameplay.contains("<Value>2</Value>"));
    assert!(!merged.gameplay.contains("<Value>9</Value>"));
}

#[test]
fn update_changes_only_the_matching_rows() {
    let merged = run_merge(
        "CREATE TABLE Defines (Name text PRIMARY KEY, Value integer); \
         INSERT INTO Defines VALUES ('X', 1); \
         INSERT INTO Defines VALUES ('Y', 1);",
        "",
        &[(
            "update.xml",
            "<GameData>\n\
             \t<Defines>\n\
             \t\t<Update>\n\
             \t\t\t<Where Name=\"X\"/>\n\
             \t\t\t<Set>\n\
             \t\t\t\t<Value>7</Value>\n\
             \t\t\t</Set>\n\
             \t\t</Update>\n\
             \t</Defines>\n\
             </GameData>",
        )],
    )
    .unwrap();

    assert!(merged.gameplay.contains("<Value>7</Value>"));
    assert!(merged.gameplay.contains("<Value>1</Value>"));
}

#[test]
fn delete_removes_matching_rows_or_everything() {
    let with_condition = run_merge(
        "CREATE TABLE Defines (Name text PRIMARY KEY, Value integer); \
         INSERT INTO Defines VALUES ('X', 1); \
         INSERT INTO Defines VALUES ('Y', 2);",
        "",
        &[(
            "delete.xml",
            "<GameData><Defines><Delete Name=\"X\"/></Defines></GameData>",
        )],
    )
    .unwrap();
    assert!(!with_condition.gameplay.contains("<Name>X</Name>"));
    assert!(with_condition.gameplay.contains("<Name>Y</Name>"));

    let delete_all = run_merge(
        "CREATE TABLE Defines (Name text PRIMARY KEY, Value integer); \
         INSERT INTO Defines VALUES ('X', 1); \
         INSERT INTO Defines VALUES ('Y', 2);",
        "",
        &[(
            "delete.xml",
            "<GameData><Defines><Delete/></Defines></GameData>",
        )],
    )
    .unwrap();
    assert!(!delete_all.gameplay.contains("<Name>"));
}

#[test]
fn language_tables_route_to_the_localization_database() {
    let merged = run_merge(
        "",
        "CREATE TABLE Language_en_US (Tag text, Text text, Gender text, Plurality text);",
        &[(
            "text.xml",
            "<GameData>\n\
             \t<Language_en_US>\n\
             \t\t<Row Tag=\"TXT_KEY_A\">\n\
             \t\t\t<Text>Hello</Text>\n\
             \t\t</Row>\n\
             \t</Language_en_US>\n\
             </GameData>",
        )],
    )
    .unwrap();

    assert!(merged.text.contains("\t\t<Replace Tag=\"TXT_KEY_A\">"));
    assert!(merged.text.contains("\t\t\t\tHello"));
    assert!(!merged.gameplay.contains("TXT_KEY_A"));
}

#[test]
fn a_duplicate_row_is_skipped_with_a_progress_line() {
    let merged = run_merge(
        "CREATE TABLE Defines (Name text PRIMARY KEY, Value integer); \
         INSERT INTO Defines VALUES ('X', 1);",
        "",
        &[(
            "dupe.xml",
            "<GameData><Defines><Row Name=\"X\" Value=\"5\"/></Defines></GameData>",
        )],
    )
    .unwrap();

    assert!(merged.gameplay.contains("<Value>1</Value>"));
    assert!(!merged.gameplay.contains("<Value>5</Value>"));
    assert!(
        merged
            .progress
            .contains(&"Skipping a duplicate Defines row in dupe.xml.".to_string()),
        "progress lines: {:?}",
        merged.progress
    );
}

#[test]
fn an_update_matching_nothing_is_not_an_error() {
    let merged = run_merge(
        "CREATE TABLE Defines (Name text PRIMARY KEY, Value integer);",
        "",
        &[(
            "update.xml",
            "<GameData>\n\
             <Defines><Update><Where Name=\"NOPE\"/><Set Value=\"9\"/></Update></Defines>\n\
             </GameData>",
        )],
    );

    assert!(merged.is_ok());
}

#[test]
fn an_update_on_a_missing_table_is_a_mod_bug() {
    let error = run_merge(
        "",
        "",
        &[(
            "broken.xml",
            "<GameData>\n\
             <Nonexistent><Update><Where A=\"1\"/><Set B=\"2\"/></Update></Nonexistent>\n\
             </GameData>",
        )],
    )
    .unwrap_err();

    assert!(error.message().contains("broken.xml"));
    assert!(error.detail().contains("Nonexistent"));
}

#[test]
fn a_root_element_other_than_gamedata_names_the_file() {
    let error = run_merge(
        "CREATE TABLE Defines (Name text);",
        "",
        &[(
            "bad.xml",
            "<GameInfo><Defines><Row Name=\"X\"/></Defines></GameInfo>",
        )],
    )
    .unwrap_err();

    assert!(error.message().contains("bad.xml"));
    assert!(error.detail().contains("GameData"));
}

#[test]
fn an_unknown_file_kind_is_skipped_and_reported() {
    let merged = run_merge("", "", &[("notes.txt", "remember to feed the elephants")]).unwrap();

    assert!(
        merged
            .progress
            .contains(&"Skipping notes.txt — not a database update.".to_string()),
        "progress lines: {:?}",
        merged.progress
    );
}

#[test]
fn a_bom_declaration_and_comments_are_tolerated() {
    let merged = run_merge(
        "CREATE TABLE Defines (Name text);",
        "",
        &[(
            "decorated.xml",
            "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
             <!-- a comment before the root -->\n\
             <GameData>\n\
             \t<!-- and one inside -->\n\
             \t<Defines><Row Name=\"X\"/></Defines>\n\
             </GameData>",
        )],
    )
    .unwrap();

    assert!(merged.gameplay.contains("<Name>X</Name>"));
}

// ---------------------------------------------------------------- the dump format

/// The full gameplay dump against a literal, because the format is the contract: table
/// ordering, the skipped table, the schema-skipped-but-data-dumped table, attribute order,
/// escaping, NULL/empty skipping, written zeros, and the blank lines the Lua's chunked
/// writer leaves behind.
#[test]
fn the_gameplay_dump_replicates_the_modpack_maker_byte_for_byte() {
    let merged = run_merge(
        "CREATE TABLE Audio_Sounds (SoundID text); \
         INSERT INTO Audio_Sounds VALUES ('AS2D_TEST'); \
         CREATE TABLE ScannedFiles (Name text); \
         INSERT INTO ScannedFiles VALUES ('ignored'); \
         CREATE TABLE Beliefs (\
             ID integer PRIMARY KEY AUTOINCREMENT, \
             Type text NOT NULL, \
             Description text DEFAULT 'TXT_KEY', \
             Enabled boolean DEFAULT 1, \
             Cost integer, \
             Ratio real); \
         INSERT INTO Beliefs (Type, Description, Enabled, Cost, Ratio) \
             VALUES ('BELIEF_A&B', '', 0, NULL, 0.5); \
         INSERT INTO Beliefs (Type, Description, Enabled, Cost, Ratio) \
             VALUES ('BELIEF_<X>', 'Words', 1, 0, 1.0); \
         CREATE TABLE Colors (Type text, Slot integer, PRIMARY KEY (Type, Slot));",
        "",
        &[],
    )
    .unwrap();

    let expected = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
        <!-- This modpack is only compatible with the Community Patch DLL -->\n\
        <GameData>\n\
        \t<Audio_Sounds>\n\
        \t\t<Delete/>\n\
        \n\
        \t\t<Row>\n\
        \t\t\t<SoundID>AS2D_TEST</SoundID>\n\
        \t\t</Row>\n\
        \t</Audio_Sounds>\n\
        \n\
        \t<Table name=\"Beliefs\">\n\
        \t\t<Column name=\"ID\" type=\"INTEGER\" primarykey=\"true\" autoincrement=\"true\"/>\n\
        \t\t<Column name=\"Type\" type=\"TEXT\" notnull=\"true\"/>\n\
        \t\t<Column name=\"Description\" type=\"TEXT\" default=\"TXT_KEY\"/>\n\
        \t\t<Column name=\"Enabled\" type=\"boolean\" default=\"true\"/>\n\
        \t\t<Column name=\"Cost\" type=\"INTEGER\"/>\n\
        \t\t<Column name=\"Ratio\" type=\"REAL\"/>\n\
        \t</Table>\n\
        \n\
        \t<Beliefs>\n\
        \t\t<Delete/>\n\
        \n\
        \t\t<Row>\n\
        \t\t\t<ID>1</ID>\n\
        \t\t\t<Type>BELIEF_A&amp;B</Type>\n\
        \t\t\t<Enabled>0</Enabled>\n\
        \t\t\t<Ratio>0.5</Ratio>\n\
        \t\t</Row>\n\
        \t\t<Row>\n\
        \t\t\t<ID>2</ID>\n\
        \t\t\t<Type>BELIEF_&lt;X&gt;</Type>\n\
        \t\t\t<Description>Words</Description>\n\
        \t\t\t<Enabled>1</Enabled>\n\
        \t\t\t<Cost>0</Cost>\n\
        \t\t\t<Ratio>1</Ratio>\n\
        \t\t</Row>\n\
        \t</Beliefs>\n\
        \n\
        \t<Table name=\"Colors\">\n\
        \t\t<Column name=\"Type\" type=\"TEXT\" primarykey=\"true\" unique=\"true\"/>\n\
        \t\t<Column name=\"Slot\" type=\"INTEGER\" primarykey=\"true\"/>\n\
        \t</Table>\n\
        \n\
        \t<Colors>\n\
        \t\t<Delete/>\n\
        \n\
        \t</Colors>\n\
        \n\
        </GameData>\n\
        \n";

    assert_eq!(merged.gameplay, expected);
}

/// The full localization dump against a literal: all ten languages in the Lua's order,
/// open/close tags even for missing tables, and the Replace/Text/Gender line shapes.
#[test]
fn the_text_dump_replicates_the_modpack_maker_byte_for_byte() {
    let merged = run_merge(
        "",
        "CREATE TABLE Language_en_US (Tag text, Text text, Gender text, Plurality text); \
         INSERT INTO Language_en_US VALUES ('TXT_KEY_HI', 'He said & left', 'masculine', NULL); \
         CREATE TABLE Language_DE_DE (Tag text, Text text, Gender text, Plurality text);",
        &[],
    )
    .unwrap();

    let expected = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
        <!-- This modpack is only compatible with the Community Patch DLL -->\n\
        <GameData>\n\
        \t<Language_en_US>\n\
        \t\t<Replace Tag=\"TXT_KEY_HI\">\n\
        \t\t\t<Text>\n\
        \t\t\t\tHe said &amp; left\n\
        \t\t\t</Text>\n\
        \t\t\t<Gender>\n\
        \t\t\t\tmasculine\n\
        \t\t\t</Gender>\n\
        \t\t</Replace>\n\
        \t</Language_en_US>\n\
        \t<Language_DE_DE>\n\
        \t</Language_DE_DE>\n\
        \t<Language_ES_ES>\n\
        \t</Language_ES_ES>\n\
        \t<Language_FR_FR>\n\
        \t</Language_FR_FR>\n\
        \t<Language_IT_IT>\n\
        \t</Language_IT_IT>\n\
        \t<Language_JA_JP>\n\
        \t</Language_JA_JP>\n\
        \t<Language_KO_KR>\n\
        \t</Language_KO_KR>\n\
        \t<Language_PL_PL>\n\
        \t</Language_PL_PL>\n\
        \t<Language_RU_RU>\n\
        \t</Language_RU_RU>\n\
        \t<Language_ZH_HANT_HK>\n\
        \t</Language_ZH_HANT_HK>\n\
        </GameData>\n";

    assert_eq!(merged.text, expected);
}

/// The same job merged twice must produce the same bytes.
#[test]
fn merging_twice_produces_identical_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let updates = [
        (
            "one.sql",
            "INSERT INTO Beliefs (Type, Cost) VALUES ('BELIEF_X', 1);",
        ),
        (
            "two.xml",
            "<GameData><Beliefs><Row Type=\"BELIEF_Y\" Cost=\"2\"/></Beliefs></GameData>",
        ),
    ];
    let job = job_in(dir.path(), &updates);
    create_db(
        &job.gameplay_base,
        "CREATE TABLE Beliefs (Type text, Cost integer);",
    );
    create_db(
        &job.text_base,
        "CREATE TABLE Language_en_US (Tag text, Text text, Gender text, Plurality text); \
         INSERT INTO Language_en_US VALUES ('TXT_KEY_A', 'Hello', NULL, NULL);",
    );
    for (name, content) in updates {
        fs::write(dir.path().join(name), content).unwrap();
    }

    let assembler = SqliteModpackAssembler::new();
    assembler
        .merge_and_dump(&job, &ProgressReporter::silent())
        .unwrap();
    let first_gameplay = fs::read(&job.gameplay_dump).unwrap();
    let first_text = fs::read(&job.text_dump).unwrap();

    assembler
        .merge_and_dump(&job, &ProgressReporter::silent())
        .unwrap();

    assert_eq!(fs::read(&job.gameplay_dump).unwrap(), first_gameplay);
    assert_eq!(fs::read(&job.text_dump).unwrap(), first_text);
}
