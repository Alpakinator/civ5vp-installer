//! Reading a mod's `.civ5proj` - the ModBuddy project upstream treats as the source of the mod's
//! `.modinfo`.
//!
//! Only what Modinfo Completion may add is read (ADR-0014): the actions, the entry points and the
//! files. Read the way upstream's `scripts/generate_modinfo.py` (`parse_civ5proj`) reads them, which
//! is the way ModBuddy's Build does, so a line added from here is the line either would write:
//!
//! - actions: `<ModActions><Action>` with `Set`, `Type` and `FileName`, in the first
//!   `<PropertyGroup>`;
//! - entry points: `<ModContent><Content>` with `Type`, `Name`, `Description` and `FileName`, in
//!   the same group, kept only with a type and a file;
//! - files: every `<ItemGroup><Content Include="…">`, imported into the VFS when its
//!   `<ImportIntoVFS>` says `True`.
//!
//! Values are kept as written - XML-escaped - because that is how they go back out.

use crate::xml_scan;

/// What a `.civ5proj` says the mod's `.modinfo` should hold, in the project's order.
#[derive(Debug, Clone, Default)]
pub(crate) struct Project {
    pub(crate) actions: Vec<ProjectAction>,
    pub(crate) entry_points: Vec<ProjectEntryPoint>,
    pub(crate) files: Vec<ProjectFile>,
}

/// `<Set><Type>FileName</Type></Set>` in the modinfo: `OnModActivated`, `UpdateDatabase`, a path.
#[derive(Debug, Clone)]
pub(crate) struct ProjectAction {
    pub(crate) set: String,
    pub(crate) kind: String,
    pub(crate) file: String,
}

/// An `<EntryPoint>`: its addin type, the file it names, and its name and description.
#[derive(Debug, Clone)]
pub(crate) struct ProjectEntryPoint {
    pub(crate) kind: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) file: String,
}

/// A `<File>` of the modinfo: the path as the project spells it, and whether the game imports it
/// into its virtual file system.
#[derive(Debug, Clone)]
pub(crate) struct ProjectFile {
    pub(crate) path: String,
    pub(crate) import: bool,
}

/// Read a `.civ5proj`'s text, or say in a few words why it is not one.
pub(crate) fn parse(text: &str) -> Result<Project, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let xml = xml_scan::mask_comments(text);
    let project = xml_scan::first(&xml, 0..xml.len(), "Project")
        .filter(|project| !project.self_closing)
        .ok_or("it has no <Project> element")?;
    // The first group, as `generate_modinfo.py` takes it: the later ones are build
    // configurations.
    let properties = xml_scan::first(&xml, project.inner.clone(), "PropertyGroup")
        .filter(|group| !group.self_closing)
        .ok_or("it has no <PropertyGroup>")?;

    let mut parsed = Project::default();
    if let Some(actions) = xml_scan::first(&xml, properties.inner.clone(), "ModActions") {
        for action in xml_scan::elements(&xml, actions.inner.clone(), "Action") {
            let set = xml_scan::child_text(&xml, &action, "Set");
            let kind = xml_scan::child_text(&xml, &action, "Type");
            let file = xml_scan::child_text(&xml, &action, "FileName");
            // The script would write `<>file</>` for a typeless one; nothing worth copying.
            if set.is_empty() || kind.is_empty() || file.is_empty() {
                continue;
            }
            parsed.actions.push(ProjectAction {
                set: set.to_owned(),
                kind: kind.to_owned(),
                file: file.to_owned(),
            });
        }
    }
    if let Some(content) = xml_scan::first(&xml, properties.inner.clone(), "ModContent") {
        for entry in xml_scan::elements(&xml, content.inner.clone(), "Content") {
            let kind = xml_scan::child_text(&xml, &entry, "Type");
            let file = xml_scan::child_text(&xml, &entry, "FileName");
            if kind.is_empty() || file.is_empty() {
                continue;
            }
            parsed.entry_points.push(ProjectEntryPoint {
                kind: kind.to_owned(),
                name: xml_scan::child_text(&xml, &entry, "Name").to_owned(),
                description: xml_scan::child_text(&xml, &entry, "Description").to_owned(),
                file: file.to_owned(),
            });
        }
    }
    for group in xml_scan::elements(&xml, project.inner.clone(), "ItemGroup") {
        for content in xml_scan::elements(&xml, group.inner.clone(), "Content") {
            let Some(path) = xml_scan::raw_attribute(&xml, &content, "Include") else {
                continue;
            };
            if path.is_empty() {
                continue;
            }
            parsed.files.push(ProjectFile {
                path: path.to_owned(),
                // Exactly `True`, as the script compares it.
                import: !content.self_closing
                    && xml_scan::child_text(&xml, &content, "ImportIntoVFS") == "True",
            });
        }
    }
    Ok(parsed)
}
