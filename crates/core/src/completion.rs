//! Modinfo Completion: a managed mod's `.modinfo`, as deployed, gains what its `.civ5proj` lists
//! and the committed file lacks - and nothing else changes (ADR-0014).
//!
//! Upstream treats the `.civ5proj` (the ModBuddy project) as the source and the `.modinfo` as a
//! build output: ModBuddy's Build writes it, and so does `scripts/generate_modinfo.py` for a
//! Release. Between Releases contributors keep the committed one in step by hand, and sometimes
//! forget. A ModBuddy user never sees that, because Build regenerates the file; this installer
//! has no ModBuddy, so it completes the file instead.
//!
//! Add only, never replace: the committed modinfo has been right while the `.civ5proj` was behind
//! (upstream's 5.2 Release lost a load-order fix by regenerating), so what is there stays where it
//! is, byte for byte. Each added line takes its place after its nearest `.civ5proj` neighbour that
//! the modinfo already has, in the format `generate_modinfo.py` writes. A modinfo that lacks
//! nothing deploys unchanged, which is what keeps a Release install identical to the official
//! installer's.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::civ5proj::{Project, ProjectAction};
use crate::claimed::ClaimedFolder;
use crate::error::InstallError;
use crate::md5::md5_hex_upper;
use crate::plan::FolderDeployment;
use crate::progress::{ProgressReporter, Stage};
use crate::xml_scan::{self, Element};

/// A managed mod's `.modinfo` as this Deployment deploys it.
#[derive(Debug, Clone)]
pub(crate) struct ManagedModinfo {
    pub(crate) claimed: ClaimedFolder,
    /// The file's name, at the top of the mod folder - where the copy puts it too.
    pub(crate) file_name: std::ffi::OsString,
    /// The completed file, or `None` when the committed one deploys unchanged.
    pub(crate) completed: Option<Vec<u8>>,
    /// What the game will read: the completed text, or the committed one.
    pub(crate) text: String,
}

/// Work out the `.modinfo` a managed mod deploys, from its folder in the Installation Source.
///
/// Read-only: the source is a Local Repo's checkout as often as the Upstream Cache, and neither is
/// ever written to (rule 6). `None` for a folder without a modinfo (the DLC folders) or one whose
/// modinfo this Deployment does not take. A `.civ5proj` that is missing or cannot be read is not a
/// failure - the committed modinfo deploys, as it did before completion existed - but it is said.
pub(crate) fn managed_modinfo(
    deployment: &FolderDeployment,
    source_folder: &Path,
    progress: &ProgressReporter,
) -> Result<Option<ManagedModinfo>, InstallError> {
    let Some(modinfo_path) = crate::modinfo::find(source_folder)? else {
        return Ok(None);
    };
    let Some(file_name) = modinfo_path.file_name().map(std::ffi::OsStr::to_os_string) else {
        return Ok(None);
    };
    if !file_name
        .to_str()
        .is_some_and(|name| deployment.selection.admits(name))
    {
        return Ok(None);
    }
    let committed = std::fs::read(&modinfo_path).map_err(|cause| InstallError::Deployment {
        action: "read",
        path: modinfo_path.clone(),
        cause,
    })?;
    let folder = deployment.claimed.folder_name();
    let as_committed = |completed: Option<Vec<u8>>| {
        let text = String::from_utf8_lossy(completed.as_deref().unwrap_or(&committed)).into_owned();
        Ok(Some(ManagedModinfo {
            claimed: deployment.claimed,
            file_name: file_name.clone(),
            completed,
            text,
        }))
    };

    let Some(project_path) = find_project(source_folder) else {
        progress.report(
            Stage::Fetch,
            format!("{folder} has no .civ5proj, so its .modinfo is installed as it is."),
        );
        return as_committed(None);
    };
    let project_name = project_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let project = match std::fs::read(&project_path)
        .map_err(|cause| cause.to_string())
        .and_then(|bytes| String::from_utf8(bytes).map_err(|_| "it is not UTF-8 text".to_owned()))
        .and_then(|text| crate::civ5proj::parse(&text))
    {
        Ok(project) => project,
        Err(why) => {
            progress.heads_up(
                Stage::Fetch,
                format!(
                    "Could not read {folder}'s {project_name} ({why}), so its .modinfo is \
                     installed as it is."
                ),
            );
            return as_committed(None);
        }
    };
    let deployed = |relative: &str| {
        deployed_file(source_folder, relative, &|top| {
            deployment.selection.admits(top)
        })
    };
    match complete(&committed, &project, &deployed) {
        Ok(None) => as_committed(None),
        Ok(Some(completion)) => {
            progress.heads_up(
                Stage::Fetch,
                format!(
                    "Completed {folder}'s .modinfo from its .civ5proj, as ModBuddy would \
                     have: added the {}.",
                    completion.added.join(", the ")
                ),
            );
            as_committed(Some(completion.bytes))
        }
        Err(why) => {
            progress.heads_up(
                Stage::Fetch,
                format!(
                    "Could not complete {folder}'s .modinfo from its .civ5proj ({why}), so it \
                     is installed as it is."
                ),
            );
            as_committed(None)
        }
    }
}

/// Write `claimed`'s completed `.modinfo`, if it has one, over the copy just made at
/// `destination` - in MODS for a Mods Deployment, inside the Modpack's stage for a Modpack one.
pub(crate) fn write_completed(
    modinfos: &[ManagedModinfo],
    claimed: ClaimedFolder,
    destination: &Path,
) -> Result<(), InstallError> {
    let completed = modinfos
        .iter()
        .filter(|modinfo| modinfo.claimed == claimed)
        .find_map(|modinfo| Some((&modinfo.file_name, modinfo.completed.as_ref()?)));
    match completed {
        Some((file_name, bytes)) => crate::tree::write_file(&destination.join(file_name), bytes),
        None => Ok(()),
    }
}

/// The mod's ModBuddy project, at the top of its folder. Sorted, so the same one every time;
/// upstream has exactly one per mod.
fn find_project(folder: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.to_ascii_lowercase().ends_with(".civ5proj"))
        })
        .collect();
    found.sort_unstable();
    found.into_iter().next()
}

/// What completing one modinfo did: the bytes to deploy, and a short phrase per added line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Completion {
    pub(crate) bytes: Vec<u8>,
    pub(crate) added: Vec<String>,
}

/// Complete `modinfo` (the committed file's bytes) from `project`.
///
/// `deployed` resolves one of the project's paths (unescaped) to the file this Deployment will
/// deploy there, or `None` when it deploys none: a line is added only for a file that exists and
/// that this configuration takes - never a listed-but-missing entry.
///
/// `Ok(None)` when nothing is missing. `Err` says why the modinfo could not be completed, and the
/// caller deploys it as committed.
pub(crate) fn complete(
    modinfo: &[u8],
    project: &Project,
    deployed: &dyn Fn(&str) -> Option<PathBuf>,
) -> Result<Option<Completion>, String> {
    const BOM: &[u8] = b"\xEF\xBB\xBF";
    let (bom, body) = match modinfo.strip_prefix(BOM) {
        Some(body) => (true, body),
        None => (false, modinfo),
    };
    let text = std::str::from_utf8(body).map_err(|_| "it is not UTF-8 text".to_owned())?;
    let xml = xml_scan::mask_comments(text);
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let root = xml_scan::first(&xml, 0..xml.len(), "Mod")
        .filter(|root| !root.self_closing)
        .ok_or("it has no <Mod> element")?;

    let mut edits = Edits::default();
    let mut added = Vec::new();
    complete_files(
        &xml, &root, project, deployed, newline, &mut edits, &mut added,
    )?;
    complete_actions(
        &xml, &root, project, deployed, newline, &mut edits, &mut added,
    );
    complete_entry_points(
        &xml, &root, project, deployed, newline, &mut edits, &mut added,
    );
    if added.is_empty() {
        return Ok(None);
    }

    let mut bytes = Vec::with_capacity(modinfo.len() + 256);
    if bom {
        bytes.extend_from_slice(BOM);
    }
    bytes.extend_from_slice(edits.apply(text).as_bytes());
    Ok(Some(Completion { bytes, added }))
}

/// One spelling for comparing paths: forward slashes, ASCII-lowercased - the game runs on
/// Windows, where neither the separator nor the case of a path matters.
fn path_key(raw: &str) -> String {
    crate::modinfo::unescape_xml(raw)
        .trim()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

/// A path as `generate_modinfo.py` writes it into a `<File>` line or an entry point.
fn forward_slashes(raw: &str) -> String {
    raw.replace('\\', "/")
}

/// `escape_xml` of `generate_modinfo.py`: applied to an entry point's name and description only.
fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn complete_files(
    xml: &str,
    root: &Element,
    project: &Project,
    deployed: &dyn Fn(&str) -> Option<PathBuf>,
    newline: &str,
    edits: &mut Edits,
    added: &mut Vec<String>,
) -> Result<(), String> {
    let section = xml_scan::first(xml, root.inner.clone(), "Files");
    let mut present: BTreeMap<String, usize> = BTreeMap::new();
    if let Some(section) = section.as_ref().filter(|section| !section.self_closing) {
        for file in xml_scan::elements(xml, section.inner.clone(), "File") {
            present
                .entry(path_key(xml_scan::text(xml, &file)))
                .or_insert(file.end);
        }
    }

    let mut neighbour: Option<usize> = None;
    let mut new_lines = Vec::new();
    let mut seen = BTreeSet::new();
    for file in &project.files {
        let key = path_key(&file.path);
        if let Some(&end) = present.get(&key) {
            neighbour = Some(end);
            continue;
        }
        if !seen.insert(key) {
            continue;
        }
        let unescaped = crate::modinfo::unescape_xml(&file.path);
        let Some(on_disk) = deployed(&unescaped) else {
            continue;
        };
        let bytes = std::fs::read(&on_disk)
            .map_err(|cause| format!("{} could not be read: {cause}", on_disk.display()))?;
        let line = format!(
            "<File md5=\"{}\" import=\"{}\">{}</File>",
            md5_hex_upper(&bytes),
            if file.import { "1" } else { "0" },
            forward_slashes(&file.path)
        );
        added.push(format!("file {}", forward_slashes(&unescaped)));
        match (neighbour, &section) {
            (Some(end), _) => edits.insert(end, format!("{newline}    {line}")),
            (None, Some(section)) if !section.self_closing => {
                edits.insert(section.inner.start, format!("{newline}    {line}"));
            }
            _ => new_lines.push(format!("    {line}")),
        }
    }
    if !new_lines.is_empty() {
        let block = section_block("Files", &new_lines, newline);
        match &section {
            Some(self_closing) => edits.replace_self_closing(xml, self_closing, block, newline),
            None => {
                let before = ["<Actions", "<EntryPoints", "</Mod>"]
                    .iter()
                    .find_map(|tag| first_tag(xml, root, tag))
                    .unwrap_or(root.inner.end);
                edits.insert_lines_before(xml, before, &block, newline);
            }
        }
    }
    Ok(())
}

/// One action set of the modinfo (`OnModActivated`, …): its name, the element, and where each
/// action in it ends, by lowercase type and path.
type ActionSet = (String, Element, BTreeMap<(String, String), usize>);

fn complete_actions(
    xml: &str,
    root: &Element,
    project: &Project,
    deployed: &dyn Fn(&str) -> Option<PathBuf>,
    newline: &str,
    edits: &mut Edits,
    added: &mut Vec<String>,
) {
    let section = xml_scan::first(xml, root.inner.clone(), "Actions");
    // Each set the modinfo has (`OnModActivated`, …): where its inside starts, and every action
    // in it by type and file.
    let mut sets: Vec<ActionSet> = Vec::new();
    if let Some(section) = section.as_ref().filter(|section| !section.self_closing) {
        for (name, set) in xml_scan::children(xml, section.inner.clone()) {
            let mut actions = BTreeMap::new();
            if !set.self_closing {
                for (kind, action) in xml_scan::children(xml, set.inner.clone()) {
                    actions
                        .entry((
                            kind.to_ascii_lowercase(),
                            path_key(xml_scan::text(xml, &action)),
                        ))
                        .or_insert(action.end);
                }
            }
            sets.push((name, set, actions));
        }
    }

    // Per set: the neighbour so far, and the lines for a set the modinfo does not have yet.
    let mut neighbours: BTreeMap<String, usize> = BTreeMap::new();
    let mut new_sets: Vec<(String, Vec<String>)> = Vec::new();
    let mut seen = BTreeSet::new();
    for action in &project.actions {
        let ProjectAction { set, kind, file } = action;
        let set_key = set.to_ascii_lowercase();
        let key = (kind.to_ascii_lowercase(), path_key(file));
        let existing = sets
            .iter()
            .find(|(name, _, _)| name.eq_ignore_ascii_case(set));
        if let Some(&end) = existing.and_then(|(_, _, actions)| actions.get(&key)) {
            neighbours.insert(set_key, end);
            continue;
        }
        if !seen.insert((set_key.clone(), key)) {
            continue;
        }
        if deployed(&crate::modinfo::unescape_xml(file)).is_none() {
            continue;
        }
        let line = format!("<{kind}>{file}</{kind}>");
        added.push(format!(
            "action {}",
            forward_slashes(&crate::modinfo::unescape_xml(file))
        ));
        match (neighbours.get(&set_key), existing) {
            (Some(&end), _) => edits.insert(end, format!("{newline}      {line}")),
            (None, Some((_, set_element, _))) if !set_element.self_closing => {
                edits.insert(set_element.inner.start, format!("{newline}      {line}"));
            }
            _ => match new_sets.iter_mut().find(|(name, _)| name == set) {
                Some((_, lines)) => lines.push(format!("      {line}")),
                None => new_sets.push((set.clone(), vec![format!("      {line}")])),
            },
        }
    }
    if new_sets.is_empty() {
        return;
    }

    let mut set_lines = Vec::new();
    for (set, lines) in &new_sets {
        set_lines.push(format!("    <{set}>"));
        set_lines.extend(lines.iter().cloned());
        set_lines.push(format!("    </{set}>"));
    }
    match section {
        Some(section) if !section.self_closing => {
            let mut block = set_lines.join(newline);
            block.push_str(newline);
            let closing = section.inner.end;
            edits.insert_lines_before(xml, closing, &block, newline);
        }
        Some(self_closing) => edits.replace_self_closing(
            xml,
            &self_closing,
            section_block("Actions", &set_lines, newline),
            newline,
        ),
        None => {
            let before = ["<EntryPoints", "</Mod>"]
                .iter()
                .find_map(|tag| first_tag(xml, root, tag))
                .unwrap_or(root.inner.end);
            edits.insert_lines_before(
                xml,
                before,
                &section_block("Actions", &set_lines, newline),
                newline,
            );
        }
    }
}

fn complete_entry_points(
    xml: &str,
    root: &Element,
    project: &Project,
    deployed: &dyn Fn(&str) -> Option<PathBuf>,
    newline: &str,
    edits: &mut Edits,
    added: &mut Vec<String>,
) {
    let section = xml_scan::first(xml, root.inner.clone(), "EntryPoints");
    let mut present: BTreeMap<(String, String), usize> = BTreeMap::new();
    if let Some(section) = section.as_ref().filter(|section| !section.self_closing) {
        for point in xml_scan::elements(xml, section.inner.clone(), "EntryPoint") {
            let kind = xml_scan::raw_attribute(xml, &point, "type").unwrap_or_default();
            let file = xml_scan::raw_attribute(xml, &point, "file").unwrap_or_default();
            present
                .entry((kind.to_ascii_lowercase(), path_key(file)))
                .or_insert(point.end);
        }
    }

    let mut neighbour: Option<usize> = None;
    let mut new_lines = Vec::new();
    let mut seen = BTreeSet::new();
    for point in &project.entry_points {
        let key = (point.kind.to_ascii_lowercase(), path_key(&point.file));
        if let Some(&end) = present.get(&key) {
            neighbour = Some(end);
            continue;
        }
        if !seen.insert(key) {
            continue;
        }
        let unescaped = crate::modinfo::unescape_xml(&point.file);
        if deployed(&unescaped).is_none() {
            continue;
        }
        let lines = [
            format!(
                "    <EntryPoint type=\"{}\" file=\"{}\">",
                point.kind,
                forward_slashes(&point.file)
            ),
            format!(
                "      <Name>{}</Name>",
                escape_xml(&crate::modinfo::unescape_xml(&point.name))
            ),
            format!(
                "      <Description>{}</Description>",
                escape_xml(&crate::modinfo::unescape_xml(&point.description))
            ),
            "    </EntryPoint>".to_owned(),
        ];
        added.push(format!("entry point {}", forward_slashes(&unescaped)));
        let inline: String = lines
            .iter()
            .map(|line| format!("{newline}{line}"))
            .collect();
        match (neighbour, &section) {
            (Some(end), _) => edits.insert(end, inline),
            (None, Some(section)) if !section.self_closing => {
                edits.insert(section.inner.start, inline);
            }
            _ => new_lines.extend(lines),
        }
    }
    if new_lines.is_empty() {
        return;
    }
    let block = section_block("EntryPoints", &new_lines, newline);
    match &section {
        Some(self_closing) => edits.replace_self_closing(xml, self_closing, block, newline),
        None => {
            let before = first_tag(xml, root, "</Mod>").unwrap_or(root.inner.end);
            edits.insert_lines_before(xml, before, &block, newline);
        }
    }
}

/// `  <name>`, the lines, `  </name>`, each line ending in `newline` - a whole section.
fn section_block(name: &str, lines: &[String], newline: &str) -> String {
    let mut block = format!("  <{name}>{newline}");
    for line in lines {
        block.push_str(line);
        block.push_str(newline);
    }
    block.push_str(&format!("  </{name}>{newline}"));
    block
}

/// Where `tag` first appears inside the root element, or its own closing tag for `</Mod>`.
fn first_tag(xml: &str, root: &Element, tag: &str) -> Option<usize> {
    if tag == "</Mod>" {
        return Some(root.inner.end);
    }
    let name = tag.trim_start_matches('<');
    xml_scan::first(xml, root.inner.clone(), name).map(|element| element.start)
}

/// The changes to make to the committed text, applied all at once so that every position still
/// refers to the committed text. Several insertions at one place keep the order they were made in.
#[derive(Default)]
struct Edits {
    /// Start, end (`== start` for an insertion), replacement.
    changes: Vec<(usize, usize, String)>,
}

impl Edits {
    fn insert(&mut self, at: usize, text: String) {
        self.changes.push((at, at, text));
    }

    /// A block of whole lines (each ending in a newline) in front of the line holding `at`.
    fn insert_lines_before(&mut self, xml: &str, at: usize, block: &str, newline: &str) {
        match xml_scan::line_start_before(xml, at) {
            Some(line_start) => self.insert(line_start, block.to_owned()),
            // `at` shares its line with something before it: start a line of our own.
            None => self.insert(at, format!("{newline}{block}")),
        }
    }

    /// Replace a self-closing `<Name />` with a whole section in its place.
    fn replace_self_closing(&mut self, xml: &str, element: &Element, block: String, newline: &str) {
        // The block carries its own indentation and line end; the tag's line already has both.
        let (start, end) = match xml_scan::line_start_before(xml, element.start) {
            Some(line_start) => {
                let end = if xml[element.end..].starts_with(newline) {
                    element.end + newline.len()
                } else {
                    element.end
                };
                (line_start, end)
            }
            None => (element.start, element.end),
        };
        self.changes.push((start, end, block));
    }

    fn apply(mut self, text: &str) -> String {
        // Stable: insertions at the same place stay in the order they were made.
        self.changes.sort_by_key(|(start, _, _)| *start);
        let mut out = String::with_capacity(text.len() + 256);
        let mut cursor = 0;
        for (start, end, replacement) in self.changes {
            if start >= cursor {
                out.push_str(&text[cursor..start]);
                cursor = start;
            }
            out.push_str(&replacement);
            cursor = cursor.max(end);
        }
        out.push_str(&text[cursor..]);
        out
    }
}

/// Resolve a project path inside `source_folder` the way the game would find it - either slash,
/// any case - and say whether the Deployment takes that file: its top-level entry admitted by
/// `admits`, and no part of it one of the standard exclusions.
pub(crate) fn deployed_file(
    source_folder: &Path,
    relative: &str,
    admits: &dyn Fn(&str) -> bool,
) -> Option<PathBuf> {
    let on_disk = crate::modinfo::resolve_case_insensitive(source_folder, relative)?;
    let inside = on_disk.strip_prefix(source_folder).ok()?;
    let mut parts = inside.components().map(|part| part.as_os_str().to_str());
    let top = parts.next()??;
    if !admits(top) || crate::tree::is_excluded(top) {
        return None;
    }
    for part in parts {
        if crate::tree::is_excluded(part?) {
            return None;
        }
    }
    Some(on_disk)
}
