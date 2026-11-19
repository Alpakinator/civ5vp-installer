//! Modinfo Completion (ADR-0014) against real upstream commits, through the Core with the real
//! database assembler. Ignored in the fast suite: it exports whole trees of a real
//! Community-Patch-DLL clone (about 360 MB each) and runs a real Vox Populi database merge.
//!
//! ```text
//! CIV5VP_CPDLL_REPO=~/Projects/Community-Patch-DLL \
//! CIV5VP_REAL_MERGE_BASE=~/.local/share/civ5vp-installer/modpack-base \
//!   cargo test --release -p civ5vp-modpack --test real_modinfo_completion -- --ignored --nocapture --test-threads 1
//! ```
//!
//! `CIV5VP_CPDLL_REPO` is any clone holding the commits below (upstream's history; it is only
//! read, with `git archive`). `CIV5VP_REAL_MERGE_BASE` holds the game's own
//! `Civ5DebugDatabase.db` and `Localization-Merged.db`, as the installer snapshots them. The
//! trees are exported under `target/tmp/real-completion/`, once per commit.
//!
//! What each commit shows (ticket 17):
//! - `Release-5.4.8`: a Release, whose modinfos `release.py` regenerated - every one deploys
//!   byte for byte, in every configuration.
//! - `62c77de6f` (#13491): `(1)` lacks the `VictoryTableChanges.sql` action and file. Completed,
//!   they are the very lines `7213fbeb3` (#13594) later added by hand, md5 and place included.
//!   That commit also fixed two SQL errors in `(2)` from #13491, which no modinfo can; with just
//!   those taken from it, the Modpack merge that died on `no such column: PreferredGrandStrategy`
//!   goes through once completed.
//! - `185b99504`: `(2)`'s modinfo had `CorporationChanges.sql` moved before
//!   `BuildingChanges.sql` while the `.civ5proj` still had the old order. Add-only keeps it.
//! - `251c3f842` (#13557): six frame `.dds` files in `(1)`'s modinfo, not yet in its
//!   `.civ5proj`. Kept.
//! - `Release-5.2`: shipped without `(4a)`'s `UI/Squads.xml` entry point (a generator bug, fixed
//!   upstream in `b17886d44`). Completion restores it.

#[path = "../../core/tests/support/mod.rs"]
mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use civ5vp_core::{
    BuildConfiguration, Core, DllSource, Eui, Flavor, FortyThreeCivs, InstallConfiguration,
    InstallError, InstallMode, InstallationSource, LuaJitEngine, MenuTheme, OpeningMovie,
    ProgressReporter,
};
use civ5vp_modpack::SqliteModpackAssembler;
use support::{FixtureModpackAssembler, FixtureSourceProvider, GameFixture, MarkerToolchainRunner};

/// The managed mods' folders, as upstream names them now.
const MANAGED: [&str; 5] = [
    "(1) Community Patch",
    "(2) Vox Populi",
    "(3a) VP - EUI Compatibility Files",
    "(3b) 43 Civs Community Patch",
    "(4a) Squads for VP",
];

fn repo() -> String {
    std::env::var("CIV5VP_CPDLL_REPO").expect("CIV5VP_CPDLL_REPO")
}

/// A file as committed at `reference`.
fn git_show(reference: &str, path: &str) -> Vec<u8> {
    let shown = Command::new("git")
        .args(["-C", &repo(), "show", &format!("{reference}:{path}")])
        .output()
        .unwrap();
    assert!(shown.status.success(), "git show {reference}:{path}");
    shown.stdout
}

/// `reference`'s tree, exported once from the clone in `CIV5VP_CPDLL_REPO`.
fn tree_at(reference: &str) -> PathBuf {
    let tree = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("real-completion")
        .join(reference);
    if tree.join(".exported").is_file() {
        return tree;
    }
    let _ = fs::remove_dir_all(&tree);
    fs::create_dir_all(&tree).unwrap();
    let mut archive = Command::new("git")
        .args(["-C", &repo(), "archive", reference])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let unpacked = Command::new("tar")
        .args(["-x", "-C"])
        .arg(&tree)
        .stdin(archive.stdout.take().unwrap())
        .status()
        .unwrap();
    assert!(archive.wait().unwrap().success() && unpacked.success());
    fs::write(tree.join(".exported"), reference).unwrap();
    tree
}

/// A copy of `reference`'s tree, named `name`, changed once by `prepare`.
fn derived_tree(reference: &str, name: &str, prepare: impl FnOnce(&Path)) -> PathBuf {
    let from = tree_at(reference);
    let tree = from.with_file_name(name);
    if tree.join(".derived").is_file() {
        return tree;
    }
    let _ = fs::remove_dir_all(&tree);
    let copied = Command::new("cp")
        .arg("-a")
        .arg(&from)
        .arg(&tree)
        .status()
        .unwrap();
    assert!(copied.success());
    prepare(&tree);
    fs::write(tree.join(".derived"), name).unwrap();
    tree
}

fn configuration(tree: &Path, flavor: Flavor, mode: InstallMode) -> InstallConfiguration {
    InstallConfiguration {
        source: InstallationSource::LocalRepo {
            path: tree.to_path_buf(),
        },
        flavor,
        forty_three_civs: FortyThreeCivs::Disabled,
        build_configuration: BuildConfiguration::Release,
        install_mode: mode,
        extra_mods: Vec::new(),
        luajit: LuaJitEngine::Stock,
        menu_theme: MenuTheme::Stock,
        opening_movie: OpeningMovie::Stock,
        dll_source: DllSource::ShippedWhenCurrent,
    }
}

const VOX_POPULI: Flavor = Flavor::VoxPopuli { eui: Eui::Disabled };

/// A Mods Deployment of `configuration` into fresh game folders. The DLL is the marker: nothing
/// here is about the DLL.
fn deploy(tree: &Path, configuration: &InstallConfiguration) -> (GameFixture, Vec<String>) {
    let game = GameFixture::new();
    let core = Core::new(
        Box::new(FixtureSourceProvider::new(tree.to_path_buf())),
        Box::new(MarkerToolchainRunner),
        Box::new(FixtureModpackAssembler::ignored()),
        game.work_dir(),
    );
    let (sender, receiver) = std::sync::mpsc::channel();
    let plan = core.plan(configuration, &game.folders()).unwrap();
    if let Err(error) = core.execute(&plan, &ProgressReporter::to_channel(sender)) {
        panic!("{} - {}", error.user_message(), error.log_detail());
    }
    let kept = receiver
        .try_iter()
        .filter(|event| event.heads_up)
        .map(|event| event.message)
        .collect();
    (game, kept)
}

/// The one `.modinfo` at the top of `folder`.
fn modinfo_in(folder: &Path) -> PathBuf {
    fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "modinfo")
        })
        .unwrap_or_else(|| panic!("no modinfo in {}", folder.display()))
}

fn deployed_modinfo(game: &GameFixture, folder: &str) -> String {
    fs::read_to_string(modinfo_in(&game.game_root().join("MODS").join(folder))).unwrap()
}

fn committed_modinfo(tree: &Path, folder: &str) -> String {
    fs::read_to_string(modinfo_in(&tree.join(folder))).unwrap()
}

/// Add-only, checked whole: every line the committed modinfo has is in the deployed one, in the
/// same order.
fn assert_add_only(committed: &str, deployed: &str, what: &str) {
    let mut deployed_lines = deployed.lines();
    for line in committed.lines() {
        assert!(
            deployed_lines.any(|candidate| candidate == line),
            "{what}: the committed line {line:?} is missing or out of order"
        );
    }
}

fn line_index(text: &str, needle: &str) -> usize {
    text.lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no line holding {needle:?}"))
}

#[test]
#[ignore = "needs a real Community-Patch-DLL clone; see the module docs"]
fn a_release_deploys_every_modinfo_byte_for_byte() {
    let tree = tree_at("Release-5.4.8");
    for (flavor, forty_three) in [
        (VOX_POPULI, FortyThreeCivs::Disabled),
        (
            Flavor::VoxPopuli { eui: Eui::Enabled },
            FortyThreeCivs::Disabled,
        ),
        (VOX_POPULI, FortyThreeCivs::Enabled),
        (Flavor::CommunityPatch, FortyThreeCivs::Disabled),
    ] {
        let mut configuration = configuration(&tree, flavor.clone(), InstallMode::Mods);
        configuration.forty_three_civs = forty_three;
        let (game, kept) = deploy(&tree, &configuration);
        for folder in MANAGED {
            if !game.game_root().join("MODS").join(folder).is_dir() {
                continue;
            }
            assert_eq!(
                deployed_modinfo(&game, folder),
                committed_modinfo(&tree, folder),
                "{folder}, {flavor:?}, 43 Civs {forty_three:?}"
            );
        }
        assert!(
            !kept.iter().any(|line| line.starts_with("Completed")),
            "{flavor:?}: {kept:?}"
        );
        println!("{flavor:?} 43 Civs {forty_three:?}: every modinfo as committed");
    }
}

#[test]
#[ignore = "needs a real Community-Patch-DLL clone; see the module docs"]
fn the_victory_table_action_is_restored_where_the_civ5proj_has_it() {
    let tree = tree_at("62c77de6f");
    let (game, kept) = deploy(&tree, &configuration(&tree, VOX_POPULI, InstallMode::Mods));
    for line in &kept {
        println!("kept: {line}");
    }
    let folder = "(1) Community Patch";
    let deployed = deployed_modinfo(&game, folder);
    assert_add_only(&committed_modinfo(&tree, folder), &deployed, folder);
    let speed = line_index(
        &deployed,
        "<UpdateDatabase>Database Changes\\GameSpeedTableChanges.sql</UpdateDatabase>",
    );
    let victory = line_index(
        &deployed,
        "<UpdateDatabase>Database Changes\\VictoryTableChanges.sql</UpdateDatabase>",
    );
    assert_eq!(victory, speed + 1, "right after its .civ5proj neighbour");

    // The lines #13594 added by hand, verbatim, md5 included. The action also follows the line
    // it follows there - action order is load order. `<Files>` order means nothing to the game:
    // the file line sits where the .civ5proj's alphabetical list has it, after
    // RemapperTables.xml, as generate_modinfo.py would put it, not where the hand fix did.
    let fixed = String::from_utf8(git_show(
        "7213fbeb3",
        "(1) Community Patch/(1) Community Patch (v 151).modinfo",
    ))
    .unwrap();
    let fixed: Vec<&str> = fixed.lines().collect();
    let deployed: Vec<&str> = deployed.lines().collect();
    let mut compared = 0;
    for (index, line) in fixed.iter().enumerate() {
        if !line.contains("VictoryTableChanges.sql") {
            continue;
        }
        let at = deployed
            .iter()
            .position(|candidate| candidate == line)
            .unwrap_or_else(|| panic!("{line:?} is not in the completed modinfo"));
        if line.contains("<UpdateDatabase>") {
            assert_eq!(deployed[at - 1], fixed[index - 1], "before {line:?}");
        }
        compared += 1;
    }
    assert_eq!(compared, 2, "one <File> and one action");
}

#[test]
#[ignore = "needs a real Community-Patch-DLL clone and a base snapshot; see the module docs"]
fn the_modpack_merge_of_62c77de6f_goes_through_once_completed() {
    // #13491's two SQL errors, fixed as #13594 fixed them; its modinfo stays as it was.
    let sql_fixed = |tree: &Path| {
        for path in [
            "(2) Vox Populi/Database Changes/City/Buildings/BuildingChanges.sql",
            "(2) Vox Populi/Database Changes/Policies/Artistry.sql",
        ] {
            fs::write(tree.join(path), git_show("7213fbeb3", path)).unwrap();
        }
    };
    let tree = derived_tree("62c77de6f", "62c77de6f-sql-fixed", sql_fixed);
    // And without (1)'s .civ5proj: nothing to complete from, so the committed modinfo goes in
    // as it is - which is what every Modpack build of master did before #13594.
    let uncompleted = derived_tree(
        "62c77de6f",
        "62c77de6f-sql-fixed-without-civ5proj",
        |tree| {
            sql_fixed(tree);
            fs::remove_file(tree.join("(1) Community Patch/Community Patch.civ5proj")).unwrap();
        },
    );
    let error = build_modpack(&uncompleted).expect_err("the merge fails without the action");
    println!("without completion: {}", error.log_detail());
    assert!(
        error.log_detail().contains("PreferredGrandStrategy"),
        "{}",
        error.log_detail()
    );

    build_modpack(&tree)
        .unwrap_or_else(|error| panic!("{} - {}", error.user_message(), error.log_detail()));
    println!("with completion: the merge went through");
}

/// A real Vox Populi Modpack of `tree` on the base snapshot in `CIV5VP_REAL_MERGE_BASE`.
fn build_modpack(tree: &Path) -> Result<(), InstallError> {
    let base = PathBuf::from(std::env::var("CIV5VP_REAL_MERGE_BASE").expect("_BASE"));
    let game = GameFixture::new();
    for file in [
        "DLC/Expansion2/UI/InGame/InGame.lua",
        "DLC/Expansion2/UI/InGame/CityView/CityView.lua",
        "DLC/Expansion2/UI/InGame/LeaderHead/LeaderHeadRoot.lua",
    ] {
        game.plant(file, "-- fixture base UI\n");
    }
    let snapshot = game.work_dir().join("modpack-base");
    fs::create_dir_all(&snapshot).unwrap();
    for name in ["Civ5DebugDatabase.db", "Localization-Merged.db"] {
        fs::copy(base.join(name), snapshot.join(name)).unwrap();
    }
    let core = Core::new(
        Box::new(FixtureSourceProvider::new(tree.to_path_buf())),
        Box::new(MarkerToolchainRunner),
        Box::new(SqliteModpackAssembler::new()),
        game.work_dir(),
    );
    let plan = core
        .plan(
            &configuration(tree, VOX_POPULI, InstallMode::Modpack),
            &game.folders(),
        )
        .unwrap();
    core.execute(&plan, &ProgressReporter::silent()).map(|_| ())
}

#[test]
#[ignore = "needs a real Community-Patch-DLL clone; see the module docs"]
fn a_load_order_fixed_in_the_modinfo_only_is_kept() {
    let tree = tree_at("185b99504");
    let (game, _) = deploy(&tree, &configuration(&tree, VOX_POPULI, InstallMode::Mods));
    let folder = "(2) Vox Populi";
    let deployed = deployed_modinfo(&game, folder);
    assert_add_only(&committed_modinfo(&tree, folder), &deployed, folder);
    assert!(
        line_index(
            &deployed,
            "Corporations/CorporationChanges.sql</UpdateDatabase>"
        ) < line_index(&deployed, "Buildings/BuildingChanges.sql</UpdateDatabase>"),
        "CorporationChanges.sql before BuildingChanges.sql, as the modinfo had it"
    );
}

#[test]
#[ignore = "needs a real Community-Patch-DLL clone; see the module docs"]
fn files_in_the_modinfo_only_are_kept() {
    let tree = tree_at("251c3f842");
    let (game, _) = deploy(&tree, &configuration(&tree, VOX_POPULI, InstallMode::Mods));
    let folder = "(1) Community Patch";
    let committed = committed_modinfo(&tree, folder);
    let deployed = deployed_modinfo(&game, folder);
    assert_add_only(&committed, &deployed, folder);
    let frames: Vec<&str> = committed
        .lines()
        .filter(|line| line.contains("<File") && line.contains("Frame") && line.contains(".dds"))
        .collect();
    assert!(frames.len() >= 6, "{frames:?}");
    for frame in frames {
        assert!(deployed.lines().any(|line| line == frame), "{frame}");
    }
}

#[test]
#[ignore = "needs a real Community-Patch-DLL clone; see the module docs"]
fn the_squads_entry_point_release_5_2_lost_is_restored() {
    let tree = tree_at("Release-5.2");
    let (game, kept) = deploy(&tree, &configuration(&tree, VOX_POPULI, InstallMode::Mods));
    for line in &kept {
        println!("kept: {line}");
    }
    let folder = "(4a) Squads for VP";
    let committed = committed_modinfo(&tree, folder);
    assert!(
        !committed.contains("UI/Squads.xml\">"),
        "the Release lacks it"
    );
    let deployed = deployed_modinfo(&game, folder);
    assert_add_only(&committed, &deployed, folder);
    assert!(
        deployed.contains("<EntryPoint type=\"InGameUIAddin\" file=\"UI/Squads.xml\">"),
        "{deployed}"
    );
}
