//! The extra-mod offer against a real MODS folder. Read only.
//!
//! ```bash
//! CIV5VP_REAL_MODS="/path/to/MODS" \\
//! CIV5VP_REAL_SOURCE=~/.local/share/civ5vp-installer/upstream-cache \\
//!   cargo test -p civ5vp-core --test real_offer -- --ignored --nocapture
//! ```
//!
//! `CIV5VP_REAL_SOURCE` is optional, and adds the notes about files shared with managed mods.

mod support;

use std::path::PathBuf;

use civ5vp_core::Core;
use support::{FixtureModpackAssembler, FixtureSourceProvider, MarkerToolchainRunner};

use civ5vp_core::{
    BuildConfiguration, DllSource, Eui, Flavor, FortyThreeCivs, InstallConfiguration, InstallMode,
    InstallationSource, LuaJitEngine, extra_mod_offer,
};

fn modpack(flavor: Flavor) -> InstallConfiguration {
    InstallConfiguration {
        source: InstallationSource::unchosen(),
        flavor,
        forty_three_civs: FortyThreeCivs::Disabled,
        build_configuration: BuildConfiguration::Release,
        install_mode: InstallMode::Modpack,
        extra_mods: Vec::new(),
        luajit: LuaJitEngine::Stock,
        menu_theme: civ5vp_core::MenuTheme::Stock,
        opening_movie: civ5vp_core::OpeningMovie::Stock,
        dll_source: DllSource::ShippedWhenCurrent,
    }
}

#[test]
#[ignore = "reads the MODS folder named by CIV5VP_REAL_MODS"]
fn the_offer_for_each_flavor_on_this_machine() {
    let mods = PathBuf::from(std::env::var("CIV5VP_REAL_MODS").expect("CIV5VP_REAL_MODS"));
    let scratch = tempfile::tempdir().unwrap();
    let core = std::env::var_os("CIV5VP_REAL_SOURCE").map(|source| {
        Core::new(
            Box::new(FixtureSourceProvider::new(PathBuf::from(source))),
            Box::new(MarkerToolchainRunner),
            Box::new(FixtureModpackAssembler::ignored()),
            scratch.path().to_path_buf(),
        )
    });
    for (label, flavor) in [
        ("Neither", Flavor::OwnModsOnly),
        ("Community Patch", Flavor::CommunityPatch),
        ("Vox Populi", Flavor::VoxPopuli { eui: Eui::Disabled }),
        ("Vox Populi + EUI", Flavor::VoxPopuli { eui: Eui::Enabled }),
    ] {
        let configuration = modpack(flavor);
        let offer = extra_mod_offer(&mods, &configuration);
        println!("== {label}\n  offered: {:?}", offer.offered);
        for refused in offer.refused {
            println!("  refused: {}: {}", refused.name, refused.reason);
        }
        // Everything offered, ticked at once: the most a player could be told.
        let everything = InstallConfiguration {
            extra_mods: offer.offered,
            ..configuration
        };
        let scan = match &core {
            Some(core) => core.extra_mod_scan(&mods, &everything.source),
            None => civ5vp_core::ExtraModScan::of(&mods),
        };
        for note in scan.offer(&everything).notes {
            println!("  note: {}: {}", note.name, note.note);
        }
    }
}
