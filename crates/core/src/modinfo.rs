//! Reading a mod's `.modinfo` — the manifest the game itself goes by.
//!
//! Shared by the Modpack assembly, which needs the database updates and UI
//! entry points, and by the Dev-mode manifest validation, which compares a Local Repo's
//! folders against the file list. A scanning parser, like the toolchain's vcxproj reader:
//! modinfos are machine-written XML, and these few shapes are all that is needed.

use std::path::{Path, PathBuf};

use crate::error::InstallError;

/// The mod's `.modinfo`, at the mod root — the only place the game looks for one.
pub(crate) fn find(mod_root: &Path) -> Result<Option<PathBuf>, InstallError> {
    let entries = std::fs::read_dir(mod_root).map_err(|cause| InstallError::Deployment {
        action: "read",
        path: mod_root.to_path_buf(),
        cause,
    })?;
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort_unstable();
    for entry in paths {
        if let Some(name) = entry.file_name().and_then(|name| name.to_str())
            && name.to_ascii_lowercase().ends_with(".modinfo")
            && entry.is_file()
        {
            return Ok(Some(entry));
        }
    }
    Ok(None)
}

/// The `id="…"` of the root `<Mod>` element.
pub(crate) fn mod_id(modinfo: &str) -> Option<String> {
    let start = modinfo.find("<Mod")?;
    let rest = &modinfo[start + "<Mod".len()..];
    let end = rest.find('>')?;
    attribute_value(&rest[..end], "id")
}

/// An `<EntryPoint>` of a modinfo: its addin type and the file it names.
pub(crate) struct EntryPoint {
    pub(crate) addin_type: String,
    pub(crate) file: String,
}

/// Every `<EntryPoint type="..." file="...">` in document order.
pub(crate) fn entry_points(modinfo: &str) -> Vec<EntryPoint> {
    let mut points = Vec::new();
    let mut rest = modinfo;
    while let Some(start) = rest.find("<EntryPoint") {
        rest = &rest[start + "<EntryPoint".len()..];
        let Some(end) = rest.find('>') else {
            break;
        };
        let attributes = &rest[..end];
        if let (Some(addin_type), Some(file)) = (
            attribute_value(attributes, "type"),
            attribute_value(attributes, "file"),
        ) {
            points.push(EntryPoint { addin_type, file });
        }
        rest = &rest[end + 1..];
    }
    points
}

/// Every `<UpdateDatabase>…</UpdateDatabase>` payload in document order.
pub(crate) fn update_database_entries(modinfo: &str) -> Vec<String> {
    element_payloads(modinfo, "UpdateDatabase")
}

/// Every `<File …>…</File>` payload — the manifest of what the game will load. The game
/// ignores anything in the mod folder that is not listed here, which is exactly what the
/// Dev-mode validation warns about.
pub(crate) fn listed_files(modinfo: &str) -> Vec<String> {
    element_payloads(modinfo, "File")
}

/// The text payloads of every `<name …>payload</name>` element, in document order.
fn element_payloads(modinfo: &str, name: &str) -> Vec<String> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut entries = Vec::new();
    let mut rest = modinfo;
    while let Some(start) = rest.find(&open) {
        rest = &rest[start + open.len()..];
        // Still inside the tag name? `<File` must not match `<Files>`.
        if rest
            .chars()
            .next()
            .is_some_and(|c| !c.is_whitespace() && c != '>' && c != '/')
        {
            continue;
        }
        let Some(tag_end) = rest.find('>') else {
            break;
        };
        if rest[..tag_end].ends_with('/') {
            // Self-closing: no payload.
            rest = &rest[tag_end + 1..];
            continue;
        }
        rest = &rest[tag_end + 1..];
        let Some(end) = rest.find(&close) else {
            break;
        };
        let payload = unescape_xml(rest[..end].trim());
        if !payload.is_empty() {
            entries.push(payload);
        }
        rest = &rest[end..];
    }
    entries
}

/// The value of `name="…"` (either quote style) inside a tag's attribute list.
pub(crate) fn attribute_value(attributes: &str, name: &str) -> Option<String> {
    let mut rest = attributes;
    loop {
        let start = rest.find(name)?;
        let after = &rest[start + name.len()..];
        // Guard against matching the tail of a longer attribute name.
        let preceded_ok = start == 0
            || rest[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_whitespace());
        let after_eq = after.trim_start();
        if preceded_ok && let Some(quoted) = after_eq.strip_prefix('=') {
            let quoted = quoted.trim_start();
            let quote = quoted.chars().next()?;
            if quote == '"' || quote == '\'' {
                let inner = &quoted[1..];
                let end = inner.find(quote)?;
                return Some(unescape_xml(&inner[..end]));
            }
        }
        rest = &rest[start + name.len()..];
    }
}

pub(crate) fn unescape_xml(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Compare a Local Repo mod folder against its own `.modinfo` file list (Dev mode).
///
/// Two mismatches, treated differently because they hurt differently:
/// - a **listed file that is gone** breaks the mod in the game, so it fails the Deployment
///   before anything is touched;
/// - an **unlisted extra file** is deployed but silently ignored by the game — the classic
///   "why does my change do nothing" of mod development — so it is said out loud in the
///   activity log and the Deployment continues.
///
/// A modinfo with no `<Files>` list (or no modinfo at all — the DLC folders) checks nothing.
pub(crate) fn validate_dev_manifest(
    folder_name: &str,
    source_folder: &Path,
    progress: &crate::progress::ProgressReporter,
) -> Result<(), InstallError> {
    let Some(path) = find(source_folder)? else {
        return Ok(());
    };
    let text = std::fs::read_to_string(&path).map_err(|cause| InstallError::Deployment {
        action: "read",
        path: path.clone(),
        cause,
    })?;
    let listed = listed_files(&text);
    if listed.is_empty() {
        return Ok(());
    }

    let missing: Vec<String> = listed
        .iter()
        .filter(|entry| resolve_case_insensitive(source_folder, entry).is_none())
        .cloned()
        .collect();
    if !missing.is_empty() {
        return Err(InstallError::ModManifestMismatch {
            folder_name: folder_name.to_owned(),
            missing,
        });
    }

    let listed_set: std::collections::HashSet<String> =
        listed.iter().map(|entry| normalized(entry)).collect();
    let mut extra = Vec::new();
    collect_unlisted(source_folder, String::new(), &listed_set, &mut extra)?;
    if !extra.is_empty() {
        let mut named: Vec<&str> = extra.iter().take(5).map(String::as_str).collect();
        let more = extra.len().saturating_sub(named.len());
        if more > 0 {
            named.push("…");
        }
        progress.report(
            crate::progress::Stage::Fetch,
            format!(
                "Heads-up: {folder_name} holds {} file(s) its .modinfo does not list, and \
                 the game will ignore them: {}{}",
                extra.len(),
                named.join(", "),
                if more > 0 {
                    format!(" and {more} more")
                } else {
                    String::new()
                },
            ),
        );
    }
    Ok(())
}

/// Files under `dir` (as `/`-relative paths) that the manifest does not list. The standard
/// exclusions and the modinfo itself are not the mod's payload and are never reported.
fn collect_unlisted(
    dir: &Path,
    prefix: String,
    listed: &std::collections::HashSet<String>,
    extra: &mut Vec<String>,
) -> Result<(), InstallError> {
    let entries = std::fs::read_dir(dir).map_err(|cause| InstallError::Deployment {
        action: "read",
        path: dir.to_path_buf(),
        cause,
    })?;
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort_unstable();
    for entry in paths {
        let Some(name) = entry.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if crate::tree::is_excluded(name) || name.to_ascii_lowercase().ends_with(".modinfo") {
            continue;
        }
        let relative = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        };
        if entry.is_dir() {
            collect_unlisted(&entry, relative, listed, extra)?;
        } else if !listed.contains(&normalized(&relative)) {
            extra.push(relative);
        }
    }
    Ok(())
}

/// One spelling for comparing manifest entries with what is on disk: forward slashes,
/// ASCII-lowercased — modinfo paths are written for Windows.
fn normalized(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}

/// `relative` resolved under `root`, matching each segment without case — modinfo paths are
/// written for Windows, where case never mattered.
pub(crate) fn resolve_case_insensitive(root: &Path, relative: &str) -> Option<PathBuf> {
    let mut current = root.to_path_buf();
    for segment in relative
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
    {
        let next = current.join(segment);
        if next.exists() {
            current = next;
            continue;
        }
        let found = std::fs::read_dir(&current).ok()?.flatten().find(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(segment))
        })?;
        current = found.path();
    }
    current.is_file().then_some(current)
}
