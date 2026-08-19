//! The source list, read from the project file at the selected Version - never hardcoded,
//! so builds don't break when upstream adds a source file.
//!
//! The reference script carries a frozen copy of this list and is already stale -
//! `stackwalker/StackWalker.cpp` exists in today's project file but not in the script -
//! which is exactly the failure mode this module exists to close.
//!
//! The file parsed is `CvGameCoreDLL_Expansion2/VoxPopuli.vcxproj`, the MSBuild project the
//! VS2013 solution and both proven clang builds compile (the spec's shorthand for it is "the
//! `.civ5proj`"). Its `<ClCompile Include="…">` items are the source list. Two quirks of the
//! real file are handled here rather than passed downstream:
//!
//! * Entries use Windows separators and Windows case - today's file says `lua\CvLuaArea.cpp`
//!   where the directory on disk is `Lua/`. Fine under MSBuild, fatal on a case-sensitive
//!   filesystem, so every entry is resolved against the directory listing case-insensitively.
//! * `_precompile.cpp` is in the list but exists to *create* the precompiled header
//!   (`PrecompiledHeader=Create`); the build compiles it separately with `/Yc`, so it is not
//!   part of the returned list.
//!
//! This is deliberately not an XML parser. The attribute grammar actually used by `.vcxproj`
//! item elements is fixed enough that scanning for `<ClCompile … Include="…"` is exact on
//! every Version, and a dependency for it would buy nothing.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::ToolchainError;

/// Where the project file lives, relative to the source root. Stable across every Version
/// the installer offers.
pub const PROJECT_FILE: &str = "CvGameCoreDLL_Expansion2/VoxPopuli.vcxproj";

/// The DLL project at one Version: what to compile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DllProject {
    /// The directory holding the project file - the directory `ClCompile` entries are
    /// relative to.
    pub project_dir: PathBuf,
    /// Every source to compile through the precompiled header, relative to `project_dir`,
    /// separators normalised to `/` and case corrected to what is on disk, in project-file
    /// order.
    pub sources: Vec<String>,
    /// Whether this Version's project file defines `STACKWALKER`.
    ///
    /// The one preprocessor definition that tracks the Version rather than the reference
    /// build: upstream added the StackWalker crash logger after the docker branch froze its
    /// flags, guarded by this define, declared in the same project file that lists its
    /// source. A newer Version does not compile without it; an older one does not compile
    /// *with* it (the guarded `#include` names a file that does not exist yet). So it is
    /// read from the Version like the source list, not transcribed like the other flags.
    pub stackwalker: bool,
}

/// Read the source list from the project file at `source_root`.
pub fn load(source_root: &Path) -> Result<DllProject, ToolchainError> {
    let project_file = source_root.join(PROJECT_FILE);
    let text = fs::read_to_string(&project_file).map_err(|error| {
        ToolchainError::new(
            "This version's sources are missing their project file, so the installer cannot \
             work out what to compile. Try a different version.",
            format!("failed to read {}: {error}", project_file.display()),
        )
    })?;

    let project_dir = project_file.parent().unwrap_or(source_root).to_path_buf();

    let mut sources = Vec::new();
    for entry in cl_compile_entries(&text) {
        let normalised = entry.replace('\\', "/");
        if normalised == "_precompile.cpp" {
            continue;
        }
        let resolved = resolve_case(&project_dir, &normalised)
            .ok_or_else(|| missing_source(source_root, &normalised))?;
        sources.push(resolved);
    }

    if sources.is_empty() {
        return Err(ToolchainError::new(
            "This version's project file lists nothing to compile, so the installer cannot \
             build its DLL. Try a different version.",
            format!("no ClCompile items found in {}", project_file.display()),
        ));
    }
    Ok(DllProject {
        project_dir,
        sources,
        stackwalker: defines_stackwalker(&text),
    })
}

/// Whether any `<PreprocessorDefinitions>` element carries the `STACKWALKER` token.
fn defines_stackwalker(text: &str) -> bool {
    let mut rest = text;
    while let Some(open) = rest.find("<PreprocessorDefinitions") {
        rest = &rest[open..];
        let Some(start) = rest.find('>') else {
            return false;
        };
        rest = &rest[start + 1..];
        let contents = rest
            .find("</PreprocessorDefinitions>")
            .map_or(rest, |end| &rest[..end]);
        if contents
            .split(';')
            .any(|token| token.trim() == "STACKWALKER")
        {
            return true;
        }
    }
    false
}

fn missing_source(source_root: &Path, entry: &str) -> ToolchainError {
    ToolchainError::new(
        "This version's project file names a source file that is not in the sources, so the \
         installer cannot build its DLL. Try a different version.",
        format!(
            "{PROJECT_FILE} lists {entry}, which does not resolve under {}",
            source_root.display()
        ),
    )
}

/// Every `Include` attribute of a `<ClCompile …>` element, in file order.
fn cl_compile_entries(text: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("<ClCompile") {
        rest = &rest[open + "<ClCompile".len()..];
        // Attributes end at the tag's closing `>`; `Include` past that belongs to something
        // else entirely.
        let Some(close) = rest.find('>') else { break };
        let (tag, after) = rest.split_at(close);
        if let Some(value) = attribute_value(tag, "Include") {
            entries.push(value.to_owned());
        }
        rest = after;
    }
    entries
}

/// The value of `name="…"` inside one tag's attribute text, if present.
fn attribute_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = tag;
    while let Some(position) = rest.find(name) {
        let after_name = &rest[position + name.len()..];
        // Guard against matching the tail of a longer attribute name.
        let preceded_by_space = rest[..position].ends_with(char::is_whitespace);
        let mut after = after_name.trim_start();
        if preceded_by_space && after.starts_with('=') {
            after = after[1..].trim_start();
            let quote = after.chars().next()?;
            if quote == '"' || quote == '\'' {
                return after[1..].split(quote).next();
            }
        }
        rest = after_name;
    }
    None
}

/// Resolve `relative` (slash-separated) under `dir`, matching each component
/// case-insensitively against what is actually on disk. Returns the corrected relative path.
fn resolve_case(dir: &Path, relative: &str) -> Option<String> {
    let mut resolved_dir = dir.to_path_buf();
    let mut corrected = Vec::new();
    let components: Vec<&str> = relative.split('/').filter(|c| !c.is_empty()).collect();
    for (index, component) in components.iter().enumerate() {
        let exact = resolved_dir.join(component);
        let name = if exact.exists() {
            (*component).to_owned()
        } else {
            let listing = fs::read_dir(&resolved_dir).ok()?;
            let wanted = component.to_lowercase();
            listing
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .find(|candidate| candidate.to_lowercase() == wanted)?
        };
        resolved_dir = resolved_dir.join(&name);
        corrected.push(name);
        let is_last = index == components.len() - 1;
        if is_last && !resolved_dir.is_file() {
            return None;
        }
    }
    Some(corrected.join("/"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Source paths compared without regard to case.
    ///
    /// The project file spells one directory `lua\` while the disk has `Lua`. On a
    /// case-sensitive filesystem `load` must hand back the disk spelling or the compiler
    /// cannot open the file. On Windows the project file's own spelling already opens it, so
    /// nothing is corrected and the asked-for spelling comes back. Both resolve, which is
    /// what these tests are about; the exact spelling is checked separately, under `cfg(unix)`,
    /// where it is the whole point.
    fn ignoring_case(sources: &[String]) -> Vec<String> {
        sources.iter().map(|source| source.to_lowercase()).collect()
    }

    /// A miniature of the real file: same element shapes, same quirks.
    const PROJECT_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Project DefaultTargets="Build" ToolsVersion="12.0" xmlns="http://schemas.microsoft.com/developer/msbuild/2003">
  <ItemGroup Label="ProjectConfigurations">
    <ProjectConfiguration Include="Debug|Win32">
      <Configuration>Debug</Configuration>
    </ProjectConfiguration>
  </ItemGroup>
  <ItemGroup>
    <ClInclude Include="CvGameCoreDLLPCH.h" />
  </ItemGroup>
  <ItemGroup>
    <ClCompile Include="CvCity.cpp" />
    <ClCompile Include="lua\CvLuaArea.cpp">
      <PrecompiledHeader Condition="'$(Configuration)|$(Platform)'=='Release|Win32'">Use</PrecompiledHeader>
    </ClCompile>
    <ClCompile Include="_precompile.cpp">
      <PrecompiledHeader Condition="'$(Configuration)|$(Platform)'=='Release|Win32'">Create</PrecompiledHeader>
    </ClCompile>
  </ItemGroup>
  <ItemGroup>
    <None Include="CvGameCoreDLL.def" />
  </ItemGroup>
</Project>
"#;

    fn write_fixture(root: &Path, project_xml: &str, sources: &[&str]) {
        let dir = root.join("CvGameCoreDLL_Expansion2");
        fs::create_dir_all(&dir).unwrap();
        for source in sources {
            let path = dir.join(source);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "// source\n").unwrap();
        }
        fs::write(dir.join("VoxPopuli.vcxproj"), project_xml).unwrap();
    }

    #[test]
    fn reads_the_source_list_with_disk_case_and_without_the_pch_creator() {
        let dir = tempfile::tempdir().unwrap();
        // On disk the directory is `Lua`, while the project file says `lua\` - the real
        // repository's exact mismatch.
        write_fixture(
            dir.path(),
            PROJECT_XML,
            &["CvCity.cpp", "Lua/CvLuaArea.cpp", "_precompile.cpp"],
        );

        let project = load(dir.path()).unwrap();

        assert_eq!(
            ignoring_case(&project.sources),
            vec!["cvcity.cpp", "lua/cvluaarea.cpp"]
        );
        // The disk spelling is what a case-sensitive filesystem needs, and correcting it is
        // this function's job there.
        #[cfg(unix)]
        assert_eq!(project.sources, vec!["CvCity.cpp", "Lua/CvLuaArea.cpp"]);
        assert_eq!(
            project.project_dir,
            dir.path().join("CvGameCoreDLL_Expansion2")
        );
        assert!(
            !project.stackwalker,
            "no PreprocessorDefinitions in this fixture"
        );
    }

    /// A Version whose project file defines `STACKWALKER` (today's do; the docker branch's
    /// does not) reports it, so the flags can follow the Version.
    #[test]
    fn the_stackwalker_define_is_read_from_the_project_file() {
        let dir = tempfile::tempdir().unwrap();
        let with_defines = PROJECT_XML.replace(
            "  <ItemGroup>\n    <ClInclude",
            r#"  <ItemDefinitionGroup Condition="'$(Configuration)|$(Platform)'=='Release|Win32'">
    <ClCompile>
      <PreprocessorDefinitions>FXS_IS_DLL;WIN32;_WINDOWS;_USRDLL;EXTERNAL_PAUSING;STACKWALKER;CVGAMECOREDLL_EXPORTS;FINAL_RELEASE;_CRT_SECURE_NO_WARNINGS;STRONG_ASSUMPTIONS;NDEBUG;%(PreprocessorDefinitions)</PreprocessorDefinitions>
    </ClCompile>
  </ItemDefinitionGroup>
  <ItemGroup>
    <ClInclude"#,
        );
        write_fixture(
            dir.path(),
            &with_defines,
            &["CvCity.cpp", "Lua/CvLuaArea.cpp"],
        );

        let project = load(dir.path()).unwrap();

        assert!(project.stackwalker);
        // The definitions block must not have leaked into the source list.
        assert_eq!(
            ignoring_case(&project.sources),
            vec!["cvcity.cpp", "lua/cvluaarea.cpp"]
        );
    }

    /// A file added at a newer Version appears without any change to the installer.
    #[test]
    fn a_source_added_to_the_project_file_is_picked_up() {
        let dir = tempfile::tempdir().unwrap();
        let with_addition = PROJECT_XML.replace(
            r#"<ClCompile Include="CvCity.cpp" />"#,
            "<ClCompile Include=\"CvCity.cpp\" />\n    <ClCompile Include=\"CvNewFeature.cpp\" />",
        );
        write_fixture(
            dir.path(),
            &with_addition,
            &[
                "CvCity.cpp",
                "CvNewFeature.cpp",
                "Lua/CvLuaArea.cpp",
                "_precompile.cpp",
            ],
        );

        let project = load(dir.path()).unwrap();

        assert_eq!(
            ignoring_case(&project.sources),
            vec!["cvcity.cpp", "cvnewfeature.cpp", "lua/cvluaarea.cpp"]
        );
    }

    #[test]
    fn a_missing_project_file_is_a_plain_sentence() {
        let dir = tempfile::tempdir().unwrap();

        let error = load(dir.path()).unwrap_err();

        assert!(error.message().contains("project file"));
        assert!(error.detail().contains("VoxPopuli.vcxproj"));
    }

    #[test]
    fn a_listed_source_missing_on_disk_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        write_fixture(dir.path(), PROJECT_XML, &["CvCity.cpp", "_precompile.cpp"]);

        let error = load(dir.path()).unwrap_err();

        assert!(error.message().contains("source file"));
        assert!(error.detail().contains("lua/CvLuaArea.cpp"));
    }

    #[test]
    fn a_project_file_with_no_sources_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let empty = r#"<Project><ItemGroup><ClInclude Include="a.h" /></ItemGroup></Project>"#;
        write_fixture(dir.path(), empty, &[]);

        let error = load(dir.path()).unwrap_err();

        assert!(error.message().contains("nothing to compile"));
    }

    #[test]
    fn attribute_scanning_is_not_fooled_by_lookalikes() {
        // `ClCompile` inside a comment-like context or an `Include` belonging to ClInclude
        // must not leak into the list; nor should a `DisableInclude="x"` attribute match.
        let tricky = r#"<Project>
  <ItemGroup>
    <ClInclude Include="NotASource.h" />
    <ClCompile DisableInclude="red-herring.cpp" Include="Real.cpp" />
  </ItemGroup>
</Project>"#;
        let dir = tempfile::tempdir().unwrap();
        write_fixture(dir.path(), tricky, &["Real.cpp"]);

        let project = load(dir.path()).unwrap();

        assert_eq!(project.sources, vec!["Real.cpp"]);
    }
}
