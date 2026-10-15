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

/// A string key lands in the hash slot PUC-Lua 5.1 gives it.
///
/// Lua 5.1 does not say in what order `next` - and so `pairs`, and the C API's `lua_next` -
/// visits a table's keys. PUC-Lua's answer for string keys is a function of the keys alone: a
/// string's slot is a hash of its characters. LuaJIT 2.1's answer is not even that. It slots a
/// string by `sid`, an ID taken from a counter that is reseeded from the operating system's
/// random source every fewer than 256 strings, so the same table built the same way iterates
/// in a different order every time the game runs.
///
/// Vox Populi depends on PUC's answer without meaning to. The game copies its API into each
/// new Lua context with `lua_next`, and the registration framework VP embeds in
/// `CvLuaSupport.cpp` watches that copy through a chain of `__newindex` listeners. The one
/// waiting after `UI` (`registrationListenerPotentiallyMatchmakingLib`) swaps itself out and
/// never stores the field that woke it, so whichever API field `lua_next` returns first is
/// lost. Under PUC-Lua that is always the same field, and not one a map script reads. Under
/// stock LuaJIT it is a draw among some seventy, and when it draws `Map`, `YieldTypes` or
/// `DirectionTypes` the map script dies part-way, no civilization gets a starting plot, and
/// the game opens on a defeat.
///
/// The fix gives `sid` the hash PUC-Lua 5.1's `luaS_newlstr` computes, so `sid & hmask` is
/// PUC's main position. Nothing needs `sid` to be unique, only immutable: every reader uses it
/// as a hash - table lookups in C, in the interpreter and in JIT-compiled code, `ir_khash`
/// (which must agree with them, and does by reading the same field), and the parser's variable
/// buckets, which compare names by pointer. The string interning table keeps LuaJIT's own
/// hash, so interning is untouched.
///
/// This alone makes the order fixed but not PUC's, because collisions still settle
/// differently. [`PUC_REHASH_ORDER`] is the other half.
const PUC_STRING_SLOTS: SourcePatch = SourcePatch {
    file: "lj_str.c",
    what: "string keys take the table slots Lua 5.1 gives them",
    anchor: "#ifndef STRID_RESEED_INTERVAL
  s->sid = g->str.id++;
#elif STRID_RESEED_INTERVAL
  if (!g->str.idreseed--) {
    uint64_t r = lj_prng_u64(&g->prng);
    g->str.id = (StrID)r;
    g->str.idreseed = (uint8_t)(r >> (64 - STRID_RESEED_INTERVAL));
  }
  s->sid = g->str.id++;
#else
  s->sid = (StrID)lj_prng_u64(&g->prng);
#endif
",
    replacement:
        "  /* Civ 5 VP Installer: hash the contents the way PUC-Lua 5.1's luaS_newlstr does, so a
  ** string lands in the table slot the shipped engine puts it in. See patches.rs. */
  {
    StrID h = (StrID)len;
    MSize step = (len >> 5) + 1, l1;
    for (l1 = len; l1 >= step; l1 -= step)
      h = h ^ ((h << 5) + (h >> 2) + (uint8_t)str[l1-1]);
    s->sid = h;
  }
",
    marker: "Civ 5 VP Installer: hash the contents the way PUC-Lua 5.1's luaS_newlstr does",
};

/// A growing table re-inserts its keys last slot first, as PUC-Lua 5.1 does.
///
/// Two keys that want the same slot cannot both have it: the second goes to the highest free
/// slot, so where keys settle depends on the order they arrive in. Growing a table re-inserts
/// every key it holds, and PUC-Lua's `resize` walks the old slots from last to first where
/// LuaJIT's `lj_tab_resize` walks them first to last. With [`PUC_STRING_SLOTS`] in but not
/// this, a table's order is the same on every run and still PUC's in only 2-10% of tables
/// (`docs/pinned-artifacts.md` §7 has the measurement), so which field the game's API copy
/// hands over first would be right by luck rather than by construction.
///
/// The rest of the insertion path - Brent's variation, the free pointer walking down, when a
/// table grows and to what size - already agrees with PUC's wherever it decides the order of
/// string keys, which is how the two edits together are enough. The one size that differs, two
/// slots where PUC makes one for a single key, holds no collision for the order to depend on.
const PUC_REHASH_ORDER: SourcePatch = SourcePatch {
    file: "lj_tab.c",
    what: "a growing table re-inserts its keys in Lua 5.1's order",
    anchor: "  if (oldhmask > 0) {  /* Reinsert pairs from old hash part. */
    global_State *g;
    uint32_t i;
    for (i = 0; i <= oldhmask; i++) {
",
    replacement: "  if (oldhmask > 0) {  /* Reinsert pairs from old hash part. */
    global_State *g;
    uint32_t i;
    /* Civ 5 VP Installer: last node first, as PUC-Lua 5.1's resize() does, so collisions
    ** settle where the shipped engine settles them. See patches.rs. */
    for (i = oldhmask+1; i-- > 0; ) {
",
    marker: "Civ 5 VP Installer: last node first, as PUC-Lua 5.1's resize() does",
};

/// Every patch, in the order they are applied.
const PATCHES: [SourcePatch; 3] = [PREFIX_RELATIVE_INSERT, PUC_STRING_SLOTS, PUC_REHASH_ORDER];

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

    /// A stand-in checkout: every file a patch edits, holding the real anchors between lines
    /// the patches must leave alone.
    fn checkout_with_stock_source() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for patch in &PATCHES {
            let path = dir.path().join(patch.file);
            let mut text = std::fs::read_to_string(&path).unwrap_or_default();
            text.push_str(&format!("before\n{}\nafter\n", patch.anchor));
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    fn read(dir: &tempfile::TempDir, patch: &SourcePatch) -> String {
        std::fs::read_to_string(dir.path().join(patch.file)).unwrap()
    }

    #[test]
    fn a_stock_checkout_is_patched() {
        let dir = checkout_with_stock_source();

        let report = apply(dir.path()).unwrap();

        assert_eq!(report.applied.len(), PATCHES.len());
        assert!(report.already_there.is_empty());
        for patch in &PATCHES {
            let text = read(&dir, patch);
            assert!(
                text.contains(patch.replacement),
                "{} was not applied",
                patch.what
            );
            assert!(text.starts_with("before\n") && text.ends_with("after\n"));
        }
        assert!(read(&dir, &PREFIX_RELATIVE_INSERT).contains("lj_tab_getint(t, i)"));
    }

    /// The checkout is fetched once and reused, so every build after the first finds its own
    /// edits. Re-applying them would be a build that fails on its second run.
    #[test]
    fn a_checkout_already_patched_is_left_alone() {
        let dir = checkout_with_stock_source();
        apply(dir.path()).unwrap();
        let once: Vec<String> = PATCHES.iter().map(|patch| read(&dir, patch)).collect();

        let report = apply(dir.path()).unwrap();

        assert_eq!(report.already_there.len(), PATCHES.len());
        assert!(report.applied.is_empty());
        let twice: Vec<String> = PATCHES.iter().map(|patch| read(&dir, patch)).collect();
        assert_eq!(once, twice, "a second run must change nothing");
    }

    /// The failure that matters: someone moves the pinned commit and an anchor is gone. A
    /// build that silently skipped the patch would ship the bug it exists to fix - and for
    /// every patch, not only the first one applied.
    #[test]
    fn a_source_the_patch_does_not_fit_is_refused() {
        for patch in &PATCHES {
            let dir = checkout_with_stock_source();
            std::fs::write(
                dir.path().join(patch.file),
                format!("a {} from some other version\n", patch.file),
            )
            .unwrap();

            let error = apply(dir.path()).unwrap_err();

            assert!(
                error.detail().contains("expected once") && error.detail().contains(patch.file),
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

    /// The marker is how a build recognises its own earlier edit. One that the stock source
    /// already contained would make every checkout look patched, and the fix would never go
    /// in - silently, which is the one way a patch must not fail.
    #[test]
    fn a_marker_is_only_ever_found_in_its_own_replacement() {
        for patch in &PATCHES {
            assert!(patch.replacement.contains(patch.marker), "{}", patch.what);
            assert!(!patch.anchor.contains(patch.marker), "{}", patch.what);
        }
    }
}
