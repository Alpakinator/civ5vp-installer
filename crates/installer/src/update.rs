//! The launch-time new-version check.
//!
//! One GET against GitHub's "latest release" endpoint, on a background thread, entirely
//! best-effort: offline, rate-limited, or unparsable all mean "say nothing" - the spec is
//! explicit that launch works offline and that there is no auto-update machinery. The only
//! thing this can ever do is show one sentence with a link.
//!
//! The repository named here is where the installer's own releases will live. It must match
//! the published repository or the check silently finds nothing - release checklist item.

/// The endpoint asked for the newest release.
pub const RELEASES_API_URL: &str =
    "https://api.github.com/repos/Alpakinator/civ5vp-installer/releases/latest";

/// Where the sentence sends the player.
pub const RELEASES_PAGE_URL: &str = "https://github.com/Alpakinator/civ5vp-installer/releases";

/// This build's own version.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Ask GitHub for the newest release and compare. `Some(tag)` means a newer installer
/// exists; `None` means anything else, including every kind of failure.
pub fn check_for_newer_release() -> Option<String> {
    let response = ureq::get(RELEASES_API_URL)
        // GitHub's API requires a User-Agent; the version in it is politeness.
        .header("user-agent", &format!("civ5vp-installer/{CURRENT_VERSION}"))
        .call()
        // The UI stays silent either way; the log still records why, so "it never
        // told me about the update" is diagnosable.
        .inspect_err(|error| crate::log_detail(&format!("update check failed: {error}")))
        .ok()?;
    let mut body = response.into_body();
    let body = body
        .read_to_string()
        .inspect_err(|error| crate::log_detail(&format!("update check failed: {error}")))
        .ok()?;
    newer_release(CURRENT_VERSION, &body)
}

/// The decision half, separated so it can be tested without a socket: given this build's
/// version and the latest-release JSON, the tag to announce - or `None`.
pub fn newer_release(current: &str, latest_json: &str) -> Option<String> {
    let tag = tag_name(latest_json)?;
    let latest = numbers(&tag);
    let ours = numbers(current);
    if latest.is_empty() {
        return None;
    }
    (latest > ours).then_some(tag)
}

/// The `tag_name` field out of GitHub's release JSON.
///
/// A field scan rather than a JSON parser: the value wanted is one string in a stable,
/// GitHub-controlled document, and a parsing dependency would buy nothing. A
/// document this does not fit yields `None`, which yields silence.
fn tag_name(json: &str) -> Option<String> {
    let key = "\"tag_name\"";
    let after_key = &json[json.find(key)? + key.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?.trim_start();
    let value = after_colon.strip_prefix('"')?;
    let end = value.find('"')?;
    let tag = &value[..end];
    // A tag with an escape in it is not one of ours; silence beats guessing.
    if tag.contains('\\') {
        return None;
    }
    Some(tag.to_owned())
}

/// The dotted numbers in a version or tag, `v` prefix and pre-release tails ignored.
fn numbers(version: &str) -> Vec<u64> {
    version
        .trim_start_matches(['v', 'V'])
        .split(['.', '-'])
        .map_while(|part| part.parse::<u64>().ok())
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn release_json(tag: &str) -> String {
        format!(
            r#"{{"url": "https://api.github.com/x", "tag_name": "{tag}", "name": "Installer {tag}", "prerelease": false}}"#
        )
    }

    #[test]
    fn a_newer_release_is_announced_by_its_tag() {
        assert_eq!(
            newer_release("0.1.0", &release_json("v0.2.0")),
            Some("v0.2.0".to_owned())
        );
        assert_eq!(
            newer_release("1.9.0", &release_json("v1.10.0")),
            Some("v1.10.0".to_owned()),
            "1.10 is newer than 1.9 as numbers, not as strings"
        );
    }

    #[test]
    fn the_same_or_an_older_release_is_silence() {
        assert_eq!(newer_release("0.2.0", &release_json("v0.2.0")), None);
        assert_eq!(newer_release("0.3.0", &release_json("v0.2.9")), None);
    }

    /// Every malformed answer is silence, never a wrong sentence.
    #[test]
    fn garbage_is_silence() {
        assert_eq!(newer_release("0.1.0", "not json at all"), None);
        assert_eq!(
            newer_release("0.1.0", r#"{"message": "rate limited"}"#),
            None
        );
        assert_eq!(newer_release("0.1.0", &release_json("weird-tag")), None);
        assert_eq!(newer_release("0.1.0", r#"{"tag_name": v0.2.0}"#), None);
    }
}
