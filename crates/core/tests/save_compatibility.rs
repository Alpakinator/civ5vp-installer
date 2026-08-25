//! Whether changing Version leaves the player's saves behind, and what is said about it.

use civ5vp_core::{SaveCompatibility, label_in_sidecar};

/// The rule Vox Populi actually follows: the first two numbers are the save format, the rest
/// are fixes within it.
#[test]
fn a_patch_release_keeps_saves_and_a_minor_release_does_not() {
    assert_eq!(
        SaveCompatibility::between("Release-5.4.4", "Release-5.4.5"),
        SaveCompatibility::Unaffected
    );
    assert_eq!(
        SaveCompatibility::between("Release-5.4.5", "Release-5.5.0").message(),
        Some("Save games are not compatible between Release-5.4.5 and Release-5.5.0.".to_owned())
    );
    assert_eq!(
        SaveCompatibility::between("Release-5.4.5", "Release-6.0.0").message(),
        Some("Save games are not compatible between Release-5.4.5 and Release-6.0.0.".to_owned())
    );
}

/// Going backwards breaks saves exactly as going forwards does - the banner is about the
/// boundary, not the direction.
#[test]
fn downgrading_across_the_line_says_the_same_thing() {
    assert!(SaveCompatibility::between("Release-5.5.0", "Release-5.4.5")
        .message()
        .is_some());
}

/// `Release-5.2` is a real tag and has no third component at all. A rule that assumed three
/// numbers would either crash on it or read it as incomparable.
#[test]
fn a_two_component_release_tag_still_compares() {
    assert_eq!(
        SaveCompatibility::between("Release-5.2", "Release-5.2.7"),
        SaveCompatibility::Unaffected
    );
    assert!(SaveCompatibility::between("Release-5.2", "Release-5.3.0")
        .message()
        .is_some());
}

/// Unofficial builds are labelled with four components, and their first two still say which
/// save format they are.
#[test]
fn an_unofficial_build_compares_on_its_first_two_numbers() {
    assert_eq!(
        SaveCompatibility::between("Release-5.4.3", "5.4.3.07"),
        SaveCompatibility::Unaffected
    );
    assert!(SaveCompatibility::between("5.4.3.07", "Release-5.5.0")
        .message()
        .is_some());
}

/// `master`, an arbitrary ref and a Local Repo carry no version. Staying silent there would
/// read as "your saves are fine", which is the one thing that cannot be promised: master sits
/// ahead of the newest Release, so moving off it is a downgrade across an invisible boundary.
#[test]
fn a_version_without_numbers_is_unknown_rather_than_safe() {
    for pair in [
        ("Release-5.4.5", "master"),
        ("master", "Release-5.4.5"),
        ("Release-5.4.5", "Local"),
        ("Release-5.4.5", "some-branch"),
    ] {
        let verdict = SaveCompatibility::between(pair.0, pair.1);
        assert_eq!(
            verdict.message(),
            Some(format!(
                "Save compatibility between {} and {} is not known.",
                pair.0, pair.1
            )),
            "{pair:?}"
        );
    }
}

/// Reinstalling the same thing is not a change, whether or not it has a number.
#[test]
fn the_same_label_says_nothing_even_when_it_has_no_version() {
    assert_eq!(
        SaveCompatibility::between("master", "master"),
        SaveCompatibility::Unaffected
    );
    assert_eq!(
        SaveCompatibility::between("Local", "Local"),
        SaveCompatibility::Unaffected
    );
}

/// The banner reads the `label` line on its own. It must not depend on the fingerprint
/// matching: the `installer` line makes every sidecar from an older release fail that test,
/// and those players are precisely the ones about to change Version.
#[test]
fn the_label_is_read_from_a_sidecar_that_no_longer_matches() {
    let sidecar = "fingerprint v3\n\
                   installer 0.1.4\n\
                   source abc123\n\
                   label Release-5.4.4\n\
                   configuration release\n\
                   dll fnv1a64:0123456789abcdef\n";

    assert_eq!(label_in_sidecar(sidecar), Some("Release-5.4.4"));
}

#[test]
fn a_sidecar_with_no_label_line_answers_nothing_rather_than_guessing() {
    assert_eq!(label_in_sidecar("fingerprint v3\ninstaller 0.1.4\n"), None);
    assert_eq!(label_in_sidecar("label \n"), None);
    assert_eq!(label_in_sidecar(""), None);
}
