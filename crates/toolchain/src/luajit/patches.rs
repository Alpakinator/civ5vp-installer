//! The edits the installer makes to the pinned LuaJIT source before building it.
//!
//! The engine this builds is not a general-purpose LuaJIT: it is a replacement for the exact
//! Lua 5.1 the game ships, dropped in under the game's own file name. Where LuaJIT and
//! PUC-Lua 5.1 disagree about behaviour the language leaves undefined, the game's mods were
//! written against PUC's answer - so an engine that answers differently is not faster, it is
//! broken. These patches close those gaps, and nothing else.
//!
//! Each is an exact-text replacement rather than a diff, and a source that does not contain
//! the expected text is an error rather than a build that quietly drops the fix. That matters
//! most exactly when it is most likely to happen: bumping the pinned LuaJIT commit.

use std::path::Path;

use crate::error::ToolchainError;

/// One exact edit to a file in the pinned checkout's `src/`.
struct SourcePatch {
    /// Path relative to the checkout's `src/`.
    file: &'static str,
    /// What it is for, in one line, for the log.
    what: &'static str,
    /// The text as the pinned commit has it. Must appear exactly once.
    anchor: &'static str,
    /// What replaces it.
    replacement: &'static str,
    /// Text that exists only once the patch is in. How a second build knows not to look for
    /// the anchor again - the checkout is fetched once and reused, patched source and all.
    marker: &'static str,
}

/// `table.insert(t, pos, v)` measures the table by its contiguous prefix.
///
/// `#t` on a table with a hole is undefined: any index whose successor is nil is a valid
/// answer, and PUC-Lua 5.1 and LuaJIT pick different ones because their tables grow
/// differently. `table.insert` uses that number to decide how far to shift, so on a holey
/// table the two engines produce *different arrays* - not merely a different length.
///
/// Vox Populi's top panel hits this. It builds the strategic-resource icons with
///
/// ```lua
/// for resource in GameInfo.Resources() do
///   if strategic then table.insert(t, resource.StrategicPriority, resource) end
/// end
/// for i, resource in ipairs(t) do ... end
/// ```
///
/// and the resources arrive in ID order (iron first) while the priorities put horses first.
/// The first insert therefore lands at position 2 of an empty table. PUC-Lua calls that table
/// empty and inserting horses at 1 gives `{horses, iron}`; LuaJIT calls it length 2, shifts
/// iron to index 3, and the next insert overwrites iron with coal. `ipairs` then stops at the
/// hole and the panel shows one icon: horses. Iron and paper are gone entirely.
///
/// The fix is to measure the prefix - the largest `n` where `t[1..n]` are all non-nil, which
/// is the smallest valid border and the one every caller means. For a table without holes it
/// is `#t` exactly, so nothing else changes: measured against PUC-Lua 5.1 and stock LuaJIT
/// across every shape of insert, the only case whose *contents* differ is the broken one.
///
/// The JIT never sees this: its recorder (`recff_table_insert`) compiles the two-argument push
/// and refuses the three-argument form, so the interpreter's C function is the only
/// implementation there is.
const PREFIX_RELATIVE_INSERT: SourcePatch = SourcePatch {
    file: "lib_table.c",
    what: "table.insert measures the table the way Lua 5.1 does",
    anchor: "    if (nargs != 3*sizeof(TValue))
      lj_err_caller(L, LJ_ERR_TABINS);
    /* NOBARRIER: This just moves existing elements around. */
    for (n = lj_lib_checkint(L, 2); i > n; i--) {",
    replacement: "    if (nargs != 3*sizeof(TValue))
      lj_err_caller(L, LJ_ERR_TABINS);
    /* Civ 5 VP Installer: measure the contiguous prefix rather than an arbitrary border,
    ** so a table with a hole shifts the way PUC-Lua 5.1 shifts it. See patches.rs. */
    {
      cTValue *tv;
      for (i = 1; (tv = lj_tab_getint(t, i)) != NULL && !tvisnil(tv); i++) ;
    }
    /* NOBARRIER: This just moves existing elements around. */
    for (n = lj_lib_checkint(L, 2); i > n; i--) {",
    marker: "Civ 5 VP Installer: measure the contiguous prefix",
};

/// Every patch, in the order they are applied.
const PATCHES: [SourcePatch; 1] = [PREFIX_RELATIVE_INSERT];

/// What one run of [`apply`] did, for the log.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PatchReport {
    /// Patches this run wrote.
    pub applied: Vec<&'static str>,
    /// Patches that were already in the source from an earlier build.
    pub already_there: Vec<&'static str>,
}

/// Make the checkout's `src` say what the engine needs it to say.
///
/// Idempotent: the LuaJIT checkout is fetched once and reused, so most builds find their own
/// earlier edits and leave them alone.
pub fn apply(src: &Path) -> Result<PatchReport, ToolchainError> {
    let mut report = PatchReport::default();
    for patch in &PATCHES {
        let path = src.join(patch.file);
        let text = std::fs::read_to_string(&path).map_err(|error| {
            ToolchainError::new(
                "The LuaJIT source the installer downloaded is not complete.",
                format!("could not read {}: {error}", path.display()),
            )
        })?;

        if text.contains(patch.marker) {
            report.already_there.push(patch.what);
            continue;
        }

        let occurrences = text.matches(patch.anchor).count();
        if occurrences != 1 {
            // Loud, and on purpose. This fires when the pinned commit moves, and the failure
            // a maintainer wants then is "your patch no longer applies", not a quiet build
            // that ships the bug the patch exists to fix.
            return Err(ToolchainError::new(
                "The LuaJIT source the installer downloaded is not the version it expects, so \
                 the engine was not built.",
                format!(
                    "the text {} patches in {} was found {occurrences} times, expected once",
                    patch.what, patch.file
                ),
            ));
        }

        let patched = text.replacen(patch.anchor, patch.replacement, 1);
        std::fs::write(&path, patched)
            .map_err(|error| crate::error::io_error("patch the LuaJIT source", &path, &error))?;
        report.applied.push(patch.what);
    }
    Ok(report)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// A stand-in `lib_table.c` holding the real anchor.
    fn checkout_with_stock_source() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let text = format!("before\n{}\nafter\n", PREFIX_RELATIVE_INSERT.anchor);
        std::fs::write(dir.path().join(PREFIX_RELATIVE_INSERT.file), text).unwrap();
        dir
    }

    #[test]
    fn a_stock_checkout_is_patched() {
        let dir = checkout_with_stock_source();

        let report = apply(dir.path()).unwrap();

        assert_eq!(report.applied.len(), 1);
        assert!(report.already_there.is_empty());
        let text = std::fs::read_to_string(dir.path().join(PREFIX_RELATIVE_INSERT.file)).unwrap();
        assert!(text.contains(PREFIX_RELATIVE_INSERT.marker));
        assert!(text.contains("lj_tab_getint(t, i)"));
        assert!(text.starts_with("before\n") && text.ends_with("after\n"));
    }

    /// The checkout is fetched once and reused, so every build after the first finds its own
    /// edits. Re-applying them would be a build that fails on its second run.
    #[test]
    fn a_checkout_already_patched_is_left_alone() {
        let dir = checkout_with_stock_source();
        apply(dir.path()).unwrap();
        let once = std::fs::read_to_string(dir.path().join(PREFIX_RELATIVE_INSERT.file)).unwrap();

        let report = apply(dir.path()).unwrap();

        assert_eq!(report.already_there.len(), 1);
        assert!(report.applied.is_empty());
        let twice = std::fs::read_to_string(dir.path().join(PREFIX_RELATIVE_INSERT.file)).unwrap();
        assert_eq!(once, twice, "a second run must change nothing");
    }

    /// The failure that matters: someone moves the pinned commit and the anchor is gone. A
    /// build that silently skipped the patch would ship the bug it exists to fix.
    #[test]
    fn a_source_the_patch_does_not_fit_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(PREFIX_RELATIVE_INSERT.file),
            "a lib_table.c from some other version\n",
        )
        .unwrap();

        let error = apply(dir.path()).unwrap_err();

        assert!(
            error.detail().contains("expected once"),
            "{}",
            error.detail()
        );
        assert!(
            error.message().contains("not the version it expects"),
            "{}",
            error.message()
        );
    }
}
