# LuaJIT Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an opt-in "Use LuaJIT" choice that builds LuaJIT 2.1 from pinned source with the installer's own bootstrapped toolchain and deploys it as the game's `lua51_Win32.dll`, backing up the stock engine and restoring it on uninstall.

**Architecture:** LuaJIT reuses the two existing boundaries rather than adding a fourth: `SourceProvider` learns to materialize the pinned LuaJIT commit, and `ToolchainRunner` learns to build it into a 32-bit Windows DLL with the same clang-cl/lld-link already used for the VP DLL. Deployment introduces the installer's first *Replaced File* — a game-owned file that is backed up before being overwritten and restored on uninstall — which is a deliberate, ADR-recorded exception to the Claimed Folders invariant.

**Tech Stack:** Rust (workspace crates `core`, `sources`, `toolchain`, `installer`), `gix` for the pinned fetch, clang-cl + lld-link targeting `i386-pc-windows-msvc`, LuaJIT 2.1 (DynASM host tools), wine on Linux hosts.

**Spec:** `docs/spec.md` (user story 28, added by Task 1) and `docs/adr/0006-replaced-files-and-the-luajit-engine.md` (written by Task 1).

## Global Constraints

- **LuaJIT pin:** commit `1edc3e52b67eaf6ce5f809be8e17d6862594b8bc` of `https://github.com/LuaJIT/LuaJIT.git` (branch `v2.1`). Pinned by commit SHA, never by tag or tarball hash — GitHub archive tarballs are not byte-stable.
- **Never enable `LUAJIT_ENABLE_LUA52COMPAT`.** Civ 5 and VP are Lua 5.1; 5.2 semantics only add divergence.
- **Target must be 32-bit x86.** `/MACHINE:X86`, `-m32`, `--target=i386-pc-windows-msvc`, `/arch:SSE2`. GC64 is x64-only and irrelevant.
- **Deployed file name is exactly `lua51_Win32.dll`**, in the Game Installation root (beside `CivilizationV_DX11.exe`). Not `lua51.dll`.
- **The stock DLL is backed up before the first replacement and never overwritten in the backup store.** A second Deployment must not back up LuaJIT over the stock engine.
- **The DLL must export every symbol the game imports.** Verified as a test, not assumed. Current required set is 80 symbols across `CivilizationV_DX11.exe`, `CvGameCoreDLLFinal Release.dll`, `CvGameDatabaseWin32Final Release.dll` and VP's `CvGameCore_Expansion2.dll`.
- **Lints:** `crates/installer/src/lib.rs` denies `clippy::unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented` — including in tests. Use `let ... else { unreachable!(...) }`.
- **No external processes** except the bootstrapped clang/lld, driven through `ToolInvoker` — and, added by this plan, wine on Linux hosts as the only way to run LuaJIT's own host tools.
- **Rust edition/toolchain:** inherited from the workspace (`version.workspace`, `edition.workspace`).

---

### Task 1: The ADR and the invariant change

The installer's central documented rule is that nothing outside the Claimed Folders is ever written. LuaJIT breaks it. That change is recorded before any code depends on it, so a reviewer meets the reasoning before the machinery.

**Files:**
- Create: `docs/adr/0006-replaced-files-and-the-luajit-engine.md`
- Modify: `CONTEXT.md:49` (the "Nothing outside them is ever touched" paragraph)
- Modify: `docs/spec.md:64` (Deployment), `docs/spec.md:85` (out of scope), and the user-story list near `docs/spec.md:42`
- Modify: `docs/pinned-artifacts.md` (new §4)

- [ ] **Step 1: Write the ADR**

Create `docs/adr/0006-replaced-files-and-the-luajit-engine.md`:

```markdown
# ADR-0006: Replaced Files, and LuaJIT as the game's Lua engine

## Status

Accepted (2026-08-17).

## Context

Civilization V loads its script engine from `lua51_Win32.dll` in the Game
Installation root. The shipped file is stock Lua 5.1.4 (PUC-Rio) — verified by
its version string — and the game, both stock DLLs and Vox Populi's own
`CvGameCore_Expansion2.dll` import 80 symbols from it, every one of them a
standard Lua 5.1 C API function. LuaJIT is ABI-compatible with Lua 5.1 by
design, so a 32-bit LuaJIT build renamed to `lua51_Win32.dll` satisfies all of
them. The community has done this since 2013.

Until now the installer has never written outside its Claimed Folders. That
invariant is what lets a player trust it beside their other mods, and it is
stated in `CONTEXT.md` and `docs/spec.md`. Replacing the engine cannot be done
inside a Claimed Folder: the file belongs to the game and lives in a directory
the installer otherwise only reads.

## Decision

Introduce a third category beside Claimed Folders and Claimed Files: the
**Replaced File**. A Replaced File is a game-owned file the installer may
overwrite, subject to three rules that Claimed things do not need:

1. The original is copied into the App Data Store *before* the first
   replacement, and that backup is never written again. A second Deployment
   must not save LuaJIT over the stock engine.
2. Uninstall restores it. Removing the installer's work must leave the game
   with the engine it shipped with.
3. It is opt-in. The default configuration replaces nothing.

The only Replaced File is `lua51_Win32.dll`.

We build LuaJIT from source with the bootstrapped toolchain rather than
shipping a prebuilt DLL, for the reasons in ADR-0001: the installer already
refuses to deploy binaries it did not compile. This also avoids depending on
the abandoned community builds — the circulating LuaJIT DLLs date from
2013–2017 and MoonJIT's repository has been archived since 2021.

## Consequences

- `GameFolders` gains the Game Installation root, which the Core was
  previously and deliberately not given.
- Uninstall becomes stateful: it needs the backup store to do its job. A
  player who deletes the App Data Store between install and uninstall keeps
  LuaJIT; the uninstall reports this rather than failing.
- Steam's "Verify integrity of game files" restores the stock DLL silently.
  The installer therefore treats a stock DLL in the game as "not deployed"
  and redeploys on the next run, rather than assuming its own last write
  survived.
- The honest performance claim is narrow. Measured community results are for
  Lua-dominated work — map generation, UI, script-heavy add-ons. Vox Populi's
  AI turn time is native C++ in `CvGameCore_Expansion2.dll` and LuaJIT cannot
  affect it. The UI must not promise faster turns.
- Mods relying on Lua 5.1's deprecated implicit `arg` table in vararg
  functions break under LuaJIT, which never implemented it. This is the known
  breakage source (InGame Editor, CivWillard, Cultural Capitals).

## Alternatives rejected

- **Ship a prebuilt LuaJIT DLL.** Contradicts ADR-0001 and would mean
  deploying unmaintained 2017 binaries.
- **Upgrade the game's SQLite too.** Not possible: SQLite 3.7.17 is statically
  linked into `CvGameDatabaseWin32Final Release.dll`, which exports 135
  mangled C++ symbols and no `sqlite3_*` entry points. There is nothing to
  bind a modern SQLite to.
- **A fourth Core boundary for LuaJIT.** The work splits cleanly along the two
  boundaries that already exist: fetching source, and compiling it.
```

- [ ] **Step 2: Amend `CONTEXT.md`**

Find the paragraph at `CONTEXT.md:49` reading "Together the Claimed Folders and Claimed Files are everything the installer may write, move, or delete. Nothing outside them is ever touched, with one exception: the game's `cache` folder, which is cleared after every Deployment." Replace with:

```markdown
Together the Claimed Folders and Claimed Files are everything the installer may
write, move, or delete, with two exceptions: the game's `cache` folder, which is
cleared after every Deployment, and the Replaced File — `lua51_Win32.dll` in the
Game Installation root, which the opt-in LuaJIT engine overwrites after copying
the original into the App Data Store. Uninstall restores it. See ADR-0006.
```

- [ ] **Step 3: Amend `docs/spec.md`**

At `docs/spec.md:64`, after the existing Deployment sentence, add:

```markdown
When the LuaJIT engine is enabled, Sync also replaces `lua51_Win32.dll` in the
Game Installation root, having first copied the original into the App Data
Store. This is the only file outside the Claimed set that Deployment writes.
Uninstall restores it from that copy.
```

Add user story 28 to the story list:

```markdown
28. As a player who wants a faster script engine, I want to opt into LuaJIT, so
    that map generation and the interface are quicker — and I want my original
    game file restored when I uninstall.
```

At `docs/spec.md:85` (out of scope), add:

```markdown
Replacing the game's SQLite, or making the game 64-bit: neither is possible.
SQLite is statically linked with no exported C API, and the game ships only
32-bit executables (already `LARGE_ADDRESS_AWARE`).
```

- [ ] **Step 4: Amend `docs/pinned-artifacts.md`**

Append:

```markdown
## §4 — LuaJIT

Pinned by commit, not by tag or tarball: GitHub's generated archives are not
byte-stable, and a commit SHA is itself the content check.

- Repository: `https://github.com/LuaJIT/LuaJIT.git`
- Commit: `1edc3e52b67eaf6ce5f809be8e17d6862594b8bc` (branch `v2.1`)
- Built as: 32-bit `i386-pc-windows-msvc` DLL, deployed as `lua51_Win32.dll`
- Never built with `LUAJIT_ENABLE_LUA52COMPAT`
```

- [ ] **Step 5: Commit**

```bash
git add docs/adr/0006-replaced-files-and-the-luajit-engine.md CONTEXT.md docs/spec.md docs/pinned-artifacts.md
git commit -m "docs: ADR-0006, Replaced Files and the LuaJIT engine"
```

---

### Task 2: `GameFolders` learns the Game Installation root

**Files:**
- Modify: `crates/core/src/claimed.rs:9-16` (struct), `:47-82` (`check`)
- Modify: `crates/core/src/detect/mod.rs:229` (`game_folders`)
- Test: `crates/core/src/claimed.rs` (in-file `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `GameFolders { mods, dlc, text, game_root }`, where `game_root` is `…/Sid Meier's Civilization V`. Every later task reads `folders.game_root`.

- [ ] **Step 1: Write the failing test**

Add to the tests module in `crates/core/src/claimed.rs`:

```rust
/// The Game Installation root is a deployment target now, not just the DLC folder's
/// grandparent, so it is checked exactly as strictly as the other three.
#[test]
fn an_empty_game_root_is_refused() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("MODS")).expect("MODS");
    std::fs::create_dir_all(root.join("Text")).expect("Text");
    std::fs::create_dir_all(root.join("DLC")).expect("DLC");
    let folders = GameFolders {
        mods: root.join("MODS"),
        text: root.join("Text"),
        dlc: root.join("DLC"),
        game_root: PathBuf::new(),
    };
    let Err(InstallError::UnusableGameFolder { which, .. }) = folders.check() else {
        panic!("an empty game root must be refused")
    };
    assert_eq!(which, "Game Installation");
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p civ5vp-core an_empty_game_root_is_refused`
Expected: FAIL — `GameFolders` has no field `game_root`.

- [ ] **Step 3: Add the field and check it**

In `crates/core/src/claimed.rs`, add to `GameFolders`:

```rust
    /// `…/Sid Meier's Civilization V` — the Game Installation root.
    ///
    /// Held rather than derived from `dlc`'s grandparent: the Replaced File
    /// (`lua51_Win32.dll`, ADR-0006) is written here, and a path that decides where a
    /// game file gets overwritten must be one detection resolved, not one Sync guessed.
    pub game_root: PathBuf,
```

In `check`, extend the loop's array to include it:

```rust
        for (which, path) in [
            ("MODS", &self.mods),
            ("DLC", &self.dlc),
            ("Text", &self.text),
            ("Game Installation", &self.game_root),
        ] {
```

- [ ] **Step 4: Populate it at the one place `GameFolders` is built**

In `crates/core/src/detect/mod.rs`, `game_folders`:

```rust
pub fn game_folders(game: &GameInstallation, documents: &DocumentsFolder) -> GameFolders {
    GameFolders {
        mods: documents.mods_folder().to_path_buf(),
        dlc: game.dlc_folder().to_path_buf(),
        text: documents.text_folder().to_path_buf(),
        game_root: game.root().to_path_buf(),
    }
}
```

- [ ] **Step 5: Fix every other construction**

Run: `cargo build --workspace --all-targets 2>&1 | grep -n 'missing field'`
For each site (test fixtures in `crates/core/tests/`, `crates/installer/tests/shell.rs`), add `game_root:` set to the fixture's game directory — the same directory whose `Assets/DLC` the fixture already uses.

- [ ] **Step 6: Run the suite**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "core: GameFolders carries the Game Installation root"
```

---

### Task 3: The `LuaJitEngine` configuration choice

**Files:**
- Modify: `crates/core/src/configuration.rs` (add enum + field on `InstallConfiguration`)
- Modify: `crates/core/src/settings.rs:212-270` (`to_text`), `:273-283` (`parse`/`read_configuration`)
- Test: `crates/core/src/settings.rs` tests module

**Interfaces:**
- Produces: `pub enum LuaJitEngine { Stock, LuaJit }` with `Default = Stock`; `InstallConfiguration::luajit: LuaJitEngine`; settings key `luajit = on|off`.

- [ ] **Step 1: Write the failing test**

Add to the tests module in `crates/core/src/settings.rs`:

```rust
/// The engine choice survives a restart, and its absence reads back as the stock engine —
/// an older settings file must not silently opt a player into replacing a game file.
#[test]
fn the_luajit_choice_round_trips_and_defaults_to_stock() {
    let mut settings = Settings::default();
    let mut configuration = sample_configuration();
    configuration.luajit = LuaJitEngine::LuaJit;
    settings.configuration = Some(configuration);

    let text = settings.to_text();
    assert!(text.contains("luajit = on"), "{text}");

    let Some(read_back) = Settings::parse(&text).configuration else {
        unreachable!("the configuration was written")
    };
    assert_eq!(read_back.luajit, LuaJitEngine::LuaJit);

    let older = text.replace("luajit = on\n", "");
    let Some(from_older) = Settings::parse(&older).configuration else {
        unreachable!("the configuration was written")
    };
    assert_eq!(from_older.luajit, LuaJitEngine::Stock);
}
```

If no `sample_configuration()` helper exists in that tests module, add one that builds an `InstallConfiguration` with `InstallationSource::UpstreamCache`, `Flavor::CommunityPatch`, `FortyThreeCivs::Disabled`, `BuildConfiguration::Release`, `InstallMode::Mods`, `extra_mods: Vec::new()`, `luajit: LuaJitEngine::Stock`.

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p civ5vp-core the_luajit_choice_round_trips`
Expected: FAIL — `LuaJitEngine` not found.

- [ ] **Step 3: Add the type and the field**

In `crates/core/src/configuration.rs`:

```rust
/// Which Lua engine the game runs.
///
/// `LuaJit` replaces the game's `lua51_Win32.dll` — the one file outside the Claimed set a
/// Deployment writes (ADR-0006). `Stock` is the default, and a default that changes nothing
/// about the game is the point: replacing a game file is always something the player asked
/// for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LuaJitEngine {
    #[default]
    Stock,
    LuaJit,
}
```

Add to `InstallConfiguration`:

```rust
    /// Whether to replace the game's Lua engine with LuaJIT. Opt-in; see ADR-0006.
    pub luajit: LuaJitEngine,
```

- [ ] **Step 4: Persist it**

In `Settings::to_text`, after the `install-mode` line:

```rust
        write_line(
            &mut text,
            "luajit",
            on_off(configuration.luajit == LuaJitEngine::LuaJit),
        );
```

In `read_configuration`, alongside the other reads:

```rust
        luajit: if values.on_off("luajit").unwrap_or(false) {
            LuaJitEngine::LuaJit
        } else {
            LuaJitEngine::Stock
        },
```

If `Values` has no `on_off` helper, use the same accessor the `forty-three-civs` line already reads with.

- [ ] **Step 5: Fix every other construction**

Run: `cargo build --workspace --all-targets 2>&1 | grep -n 'missing field'`
Add `luajit: LuaJitEngine::Stock` to each.

- [ ] **Step 6: Run tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "core: opt-in LuaJIT engine choice, remembered between runs"
```

---

### Task 4: Materializing the pinned LuaJIT source

**Files:**
- Modify: `crates/core/src/boundaries.rs:67-96` (`SourceProvider`)
- Create: `crates/sources/src/luajit.rs`
- Modify: `crates/sources/src/lib.rs` (declare + re-export)
- Modify: `crates/installer/src/placeholder.rs` (the fast suite's fake provider)
- Test: `crates/sources/src/luajit.rs` tests module

**Interfaces:**
- Consumes: `GameFolders.game_root` (Task 2) is not used here.
- Produces: `SourceProvider::materialize_luajit(&self, progress: &ProgressReporter) -> Result<PathBuf, BoundaryError>` returning the LuaJIT source root (the directory holding `src/` and `dynasm/`); `pub const LUAJIT_URL: &str`; `pub const LUAJIT_COMMIT: &str`.

- [ ] **Step 1: Write the failing test**

Create `crates/sources/src/luajit.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// The pin is a commit, never a tag: tags move, and GitHub's generated tarballs are not
    /// byte-stable, so the commit SHA is the only self-verifying identity available.
    #[test]
    fn the_pin_is_a_full_commit_sha() {
        assert_eq!(LUAJIT_COMMIT.len(), 40);
        assert!(LUAJIT_COMMIT.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(LUAJIT_URL.ends_with("LuaJIT.git"));
    }

    /// A cache that already holds the pinned commit is reused rather than refetched — the
    /// same rule the Upstream Cache follows, and what keeps a second Deployment offline.
    #[test]
    fn an_existing_checkout_is_reused() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let cache = LuaJitCache::new(dir.path().join("luajit-cache"));
        std::fs::create_dir_all(cache.source_root().join("src")).expect("src");
        std::fs::create_dir_all(cache.source_root().join("dynasm")).expect("dynasm");
        std::fs::write(cache.stamp_path(), LUAJIT_COMMIT).expect("stamp");
        assert!(cache.already_has_the_pinned_commit());
    }
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p civ5vp-sources luajit`
Expected: FAIL — module not declared.

- [ ] **Step 3: Implement the cache**

Prepend to `crates/sources/src/luajit.rs`:

```rust
//! The pinned LuaJIT checkout: one commit, fetched once, reused forever.
//!
//! Pinned by commit rather than tag — see `docs/pinned-artifacts.md` §4. A stamp file
//! beside the checkout records which commit is on disk, so a second Deployment needs no
//! network at all.

use std::path::{Path, PathBuf};

use civ5vp_core::{BoundaryError, ProgressReporter, Stage};

/// Upstream LuaJIT. The only LuaJIT URL in the crate.
pub const LUAJIT_URL: &str = "https://github.com/LuaJIT/LuaJIT.git";

/// The pinned commit on branch `v2.1` (`docs/pinned-artifacts.md` §4).
pub const LUAJIT_COMMIT: &str = "1edc3e52b67eaf6ce5f809be8e17d6862594b8bc";

const STAMP_FILE_NAME: &str = ".luajit-commit";

/// The LuaJIT checkout inside the App Data Store.
pub struct LuaJitCache {
    root: PathBuf,
}

impl LuaJitCache {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Where the built source tree lives: the directory holding `src/` and `dynasm/`.
    pub fn source_root(&self) -> PathBuf {
        self.root.join("LuaJIT")
    }

    pub(crate) fn stamp_path(&self) -> PathBuf {
        self.root.join(STAMP_FILE_NAME)
    }

    /// Whether the checkout on disk is the pinned commit, with the two directories the
    /// build reads actually present.
    pub(crate) fn already_has_the_pinned_commit(&self) -> bool {
        let source = self.source_root();
        source.join("src").is_dir()
            && source.join("dynasm").is_dir()
            && std::fs::read_to_string(self.stamp_path())
                .is_ok_and(|stamp| stamp.trim() == LUAJIT_COMMIT)
    }

    /// Fetch the pinned commit if it is not already here, and return the source root.
    pub fn materialize(&self, progress: &ProgressReporter) -> Result<PathBuf, BoundaryError> {
        if self.already_has_the_pinned_commit() {
            progress.report(Stage::Fetch, "LuaJIT source is already here.");
            return Ok(self.source_root());
        }
        progress.report(Stage::Fetch, "Fetching the LuaJIT source.");
        self.fetch_pinned_commit()?;
        std::fs::write(self.stamp_path(), LUAJIT_COMMIT).map_err(|error| {
            BoundaryError::new(
                "The installer could not record which LuaJIT version it fetched.",
                error.to_string(),
            )
        })?;
        Ok(self.source_root())
    }
}
```

- [ ] **Step 4: Implement `fetch_pinned_commit` with `gix`**

Add to `impl LuaJitCache`, mirroring the shallow-fetch shape already used in `crates/sources/src/upstream.rs` (read `open_or_init` and the `Shallow`/`Tags` usage there and follow it exactly — same error mapping, same `Shallow::DepthAtRemote` of 1, same checkout of the fetched commit into the worktree). The one difference is that the ref fetched is the pinned commit rather than a tag chosen at runtime.

- [ ] **Step 5: Add the boundary method**

In `crates/core/src/boundaries.rs`, add to `trait SourceProvider`:

```rust
    /// The pinned LuaJIT source tree, fetched if it is not cached yet.
    ///
    /// Only called when the configuration opts into the LuaJIT engine, so a player on the
    /// stock engine never fetches it. Returns the directory holding `src/` and `dynasm/`.
    fn materialize_luajit(&self, progress: &ProgressReporter)
    -> Result<PathBuf, BoundaryError>;
```

Implement it on `InstallationSources` (delegating to `LuaJitCache`) and on the fast suite's fake in `crates/installer/src/placeholder.rs` (returning a temp directory containing empty `src/` and `dynasm/`).

- [ ] **Step 6: Run tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "sources: fetch the pinned LuaJIT commit into the App Data Store"
```

---

### Task 5: Running LuaJIT's host tools

LuaJIT's build generates its VM with two programs it compiles first — `minilua` and `buildvm` — and refuses to cross-build unless they have the *target's* pointer size. Building them as 32-bit Windows executables with the toolchain we already have makes that true by construction; on Linux they then need wine to run.

**Files:**
- Create: `crates/toolchain/src/luajit/host.rs`
- Create: `crates/toolchain/src/luajit/mod.rs`
- Modify: `crates/toolchain/src/lib.rs` (declare the module)
- Test: `crates/toolchain/src/luajit/host.rs` tests module

**Interfaces:**
- Consumes: `ToolCommand { program, args, current_dir }` and `trait ToolInvoker` from `crates/toolchain/src/build/invoke.rs:19,37`.
- Produces: `pub enum HostRunner { Native, Wine(PathBuf) }` with `HostRunner::for_this_host(steam_roots: &[PathBuf]) -> Result<Self, ToolchainError>` and `HostRunner::command(&self, exe: &Path, args: Vec<String>, current_dir: &Path) -> ToolCommand`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// On Windows the host tools are the target's own architecture and simply run; wrapping
    /// them in anything would be wrong.
    #[test]
    fn a_native_runner_invokes_the_executable_directly() {
        let runner = HostRunner::Native;
        let command = runner.command(
            Path::new("/work/buildvm.exe"),
            vec!["-m".into(), "peobj".into()],
            Path::new("/work"),
        );
        assert_eq!(command.program, PathBuf::from("/work/buildvm.exe"));
        assert_eq!(command.args, vec!["-m".to_owned(), "peobj".to_owned()]);
    }

    /// On Linux the same win32 executable runs under wine, which becomes the program and
    /// takes the executable as its first argument.
    #[test]
    fn a_wine_runner_puts_the_executable_first_among_the_arguments() {
        let runner = HostRunner::Wine(PathBuf::from("/usr/bin/wine"));
        let command = runner.command(
            Path::new("/work/buildvm.exe"),
            vec!["-m".into(), "peobj".into()],
            Path::new("/work"),
        );
        assert_eq!(command.program, PathBuf::from("/usr/bin/wine"));
        assert_eq!(
            command.args,
            vec![
                "/work/buildvm.exe".to_owned(),
                "-m".to_owned(),
                "peobj".to_owned()
            ]
        );
    }
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p civ5vp-toolchain host_runner`
Expected: FAIL — module not declared.

- [ ] **Step 3: Implement**

```rust
//! Running LuaJIT's own build tools.
//!
//! LuaJIT generates its interpreter with two programs it compiles first, `minilua` and
//! `buildvm`, and refuses to cross-build unless they have the target's pointer size
//! ("pointer size mismatch in cross-build"). Rather than require a 32-bit host compiler —
//! multilib, which many Linux installs lack — we build them as 32-bit *Windows*
//! executables with the toolchain already bootstrapped, which makes the pointer size match
//! by construction. On Windows they then run natively; on Linux they run under wine.
//!
//! Wine is a safe thing to require here: on Linux, Civilization V itself only runs through
//! Proton, so every player this installer serves already has one.

use std::path::{Path, PathBuf};

use crate::build::invoke::ToolCommand;
use crate::error::ToolchainError;

/// How to start a 32-bit Windows executable on this host.
pub enum HostRunner {
    /// Windows: run it.
    Native,
    /// Linux: run it under this wine binary.
    Wine(PathBuf),
}

impl HostRunner {
    /// Wrap one invocation.
    pub fn command(&self, exe: &Path, args: Vec<String>, current_dir: &Path) -> ToolCommand {
        match self {
            Self::Native => ToolCommand {
                program: exe.to_path_buf(),
                args,
                current_dir: current_dir.to_path_buf(),
            },
            Self::Wine(wine) => {
                let mut all = vec![exe.to_string_lossy().into_owned()];
                all.extend(args);
                ToolCommand {
                    program: wine.clone(),
                    args: all,
                    current_dir: current_dir.to_path_buf(),
                }
            }
        }
    }
}
```

- [ ] **Step 4: Add the discovery**

Add to `impl HostRunner`:

```rust
    /// Pick a runner for this host. `steam_roots` are the Steam libraries detection already
    /// found — Proton ships a wine, so a player with no system wine still has one.
    pub fn for_this_host(steam_roots: &[PathBuf]) -> Result<Self, ToolchainError> {
        if cfg!(windows) {
            return Ok(Self::Native);
        }
        if let Some(wine) = proton_wine(steam_roots).or_else(system_wine) {
            return Ok(Self::Wine(wine));
        }
        Err(ToolchainError::missing_wine())
    }
```

Implement `proton_wine` to scan `<steam root>/steamapps/common/Proton*/files/bin/wine` and take the newest by directory name, and `system_wine` to check `PATH` entries for a `wine` file. Add a `ToolchainError::missing_wine()` variant whose user message reads: `"LuaJIT needs wine to build on Linux, and none was found. Install wine, or turn the LuaJIT option off."`

- [ ] **Step 5: Contain the wine prefix**

Wine run with no `WINEPREFIX` creates `~/.wine` and pops up a Mono installer. Extend `ToolCommand` with an `env: Vec<(String, String)>` field (defaulting to empty everywhere else), and have the wine branch set:

```rust
                    env: vec![
                        ("WINEPREFIX".into(), prefix.to_string_lossy().into_owned()),
                        // No Mono, no Gecko: nothing here is .NET or HTML, and without this
                        // wine shows the user an installer dialog for both.
                        ("WINEDLLOVERRIDES".into(), "mscoree,mshtml=".into()),
                        ("WINEDEBUG".into(), "-all".into()),
                    ],
```

where `prefix` is a directory inside the Toolchain Cache. Apply `env` in `ProcessInvoker::run`.

- [ ] **Step 6: Run tests**

Run: `cargo test -p civ5vp-toolchain`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "toolchain: run LuaJIT's win32 host tools natively or under wine"
```

---

### Task 6: Building the LuaJIT DLL

**Files:**
- Create: `crates/toolchain/src/luajit/build.rs`
- Modify: `crates/toolchain/src/luajit/mod.rs`
- Modify: `crates/core/src/boundaries.rs` (`ToolchainRunner`)
- Modify: `crates/toolchain/src/runner.rs` (`BootstrappedToolchain`)
- Modify: `crates/installer/src/placeholder.rs` (fake runner)
- Test: `crates/toolchain/src/luajit/build.rs` tests module

**Interfaces:**
- Consumes: `HostRunner` (Task 5); `Toolchain::clang_path()`, `lld_link_path()`, `include_dirs()`, `lib_dirs()` from `crates/toolchain/src/cache.rs:43-77`.
- Produces: `ToolchainRunner::build_luajit(&self, request: &LuaJitBuildRequest, progress: &ProgressReporter) -> Result<(), BoundaryError>`, with `pub struct LuaJitBuildRequest { pub source_root: PathBuf, pub output_path: PathBuf }`. `output_path` is where the finished `lua51_Win32.dll` is written — always inside the Core's build directory, never the game.

- [ ] **Step 1: Write the failing test**

The whole sequence is driven through `ToolInvoker`, so it can be asserted without compiling anything. Follow the fake-invoker pattern already in `crates/toolchain/src/build/mod.rs:704-718`.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// The order is not incidental: dynasm must run before buildvm is compiled, because
    /// buildvm includes the header dynasm writes; and every generated header must exist
    /// before the library sources that include them are compiled.
    #[test]
    fn the_build_runs_dynasm_then_buildvm_then_the_library() {
        let invoker = RecordingInvoker::new();
        let plan = luajit_commands(&fake_toolchain(), &HostRunner::Native, Path::new("/lj"));
        let programs: Vec<String> = plan
            .iter()
            .map(|c| c.program.file_name().unwrap_or_default().to_string_lossy().into_owned())
            .collect();
        let dynasm = programs.iter().position(|p| p.contains("minilua")).expect("minilua runs");
        let buildvm = programs.iter().position(|p| p.contains("buildvm")).expect("buildvm runs");
        assert!(dynasm < buildvm, "dynasm generates the header buildvm needs: {programs:?}");
        let _ = invoker;
    }

    /// Lua 5.2 compatibility is never enabled: Civ 5 and Vox Populi are Lua 5.1, and 5.2
    /// semantics would only add divergence from the engine they were written against.
    #[test]
    fn lua52_compatibility_is_never_enabled() {
        let plan = luajit_commands(&fake_toolchain(), &HostRunner::Native, Path::new("/lj"));
        for command in &plan {
            for argument in &command.args {
                assert!(
                    !argument.contains("LUAJIT_ENABLE_LUA52COMPAT"),
                    "Lua 5.2 compatibility must stay off: {argument}"
                );
            }
        }
    }

    /// A 64-bit DLL would not load into a 32-bit game at all.
    #[test]
    fn everything_targets_32_bit_x86() {
        let plan = luajit_commands(&fake_toolchain(), &HostRunner::Native, Path::new("/lj"));
        let link = plan.last().expect("a link step");
        assert!(link.args.iter().any(|a| a == "/MACHINE:X86"));
        assert!(link.args.iter().any(|a| a.contains("lua51_Win32.dll")));
    }
}
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test -p civ5vp-toolchain luajit`
Expected: FAIL — `luajit_commands` not found.

- [ ] **Step 3: Implement the command plan**

Write `luajit_commands(toolchain: &Toolchain, host: &HostRunner, source_root: &Path) -> Vec<ToolCommand>` producing, with `src/` as the working directory:

1. `clang-cl` compiling `host/minilua.c`, then `lld-link /SUBSYSTEM:CONSOLE` → `minilua.exe`
2. `host.command(minilua.exe, ["../dynasm/dynasm.lua", "-LN", "-D", "WIN", "-D", "JIT", "-D", "FFI", "-D", "ENDIAN_LE", "-D", "FPU", "-o", "host/buildvm_arch.h", "vm_x86.dasc"])`
3. `host.command(minilua.exe, ["host/genversion.lua"])`
4. `clang-cl` compiling `host/buildvm.c host/buildvm_asm.c host/buildvm_fold.c host/buildvm_lib.c host/buildvm_peobj.c` with `-I.` and `-I../dynasm`, then link → `buildvm.exe`
5. `host.command(buildvm.exe, …)` seven times: `-m peobj -o lj_vm.obj`; `-m bcdef -o lj_bcdef.h <ALL_LIB>`; `-m ffdef`; `-m libdef`; `-m recdef`; `-m vmdef -o jit/vmdef.lua`; `-m folddef -o lj_folddef.h lj_opt_fold.c`
6. `clang-cl` compiling `lj_*.c lib_*.c` with `-m32 --target=i386-pc-windows-msvc /O2 /arch:SSE2 /MD /DLUA_BUILD_AS_DLL /D_CRT_SECURE_NO_DEPRECATE /D_CRT_STDIO_INLINE=__declspec(dllexport)__inline` and `-imsvc` for each `toolchain.include_dirs()`
7. `lld-link /DLL /MACHINE:X86 /OUT:lua51_Win32.dll /OPT:REF /OPT:ICF /INCREMENTAL:NO` with `/LIBPATH:` for each `toolchain.lib_dirs()`, over `lj_*.obj lib_*.obj` plus `lj_vm.obj`

where `ALL_LIB` is exactly:

```rust
const ALL_LIB: [&str; 12] = [
    "lib_base.c", "lib_math.c", "lib_bit.c", "lib_string.c", "lib_table.c", "lib_io.c",
    "lib_os.c", "lib_package.c", "lib_debug.c", "lib_jit.c", "lib_ffi.c", "lib_buffer.c",
];
```

Note: `lj_*.c` must be expanded by reading the directory, not passed as a glob — the invoker does not go through a shell.

- [ ] **Step 4: Wire the boundary**

Add `build_luajit` to `ToolchainRunner` in `crates/core/src/boundaries.rs` with the doc comment "Compile LuaJIT and write the DLL to `LuaJitBuildRequest::output_path`. Never writes into a game folder." Implement it on `BootstrappedToolchain` (bootstrap, resolve `HostRunner`, run the plan, copy the result to `output_path`) and on the fast suite's fake (write a small placeholder file).

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 6: Prove it against the real thing (manual, once)**

Run a real build via the `#[ignore]`d real-install test path, then confirm the result is a 32-bit PE that satisfies the game:

Expected: the DLL reports `machine=0x14c`, and every symbol imported from `lua51_Win32.dll` by the game's binaries is among its exports.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "toolchain: build LuaJIT 2.1 as a 32-bit lua51_Win32.dll"
```

---

### Task 7: The Replaced File — backup and restore

**Files:**
- Create: `crates/core/src/replaced.rs`
- Modify: `crates/core/src/lib.rs` (declare + re-export)
- Test: `crates/core/src/replaced.rs` tests module

**Interfaces:**
- Consumes: `GameFolders.game_root` (Task 2); `crates/core/src/tree.rs` helpers `copy_file`, `create_dir_all`, `remove_file_if_present`.
- Produces:
  - `pub enum ReplacedFile { LuaEngine }` with `file_name() -> &'static str` (`"lua51_Win32.dll"`) and `path_in(&GameFolders) -> PathBuf`
  - `pub struct BackupStore { root: PathBuf }` with `new(root: PathBuf)`, `back_up_once(&self, file: ReplacedFile, from: &Path) -> Result<(), InstallError>`, `restore(&self, file: ReplacedFile, to: &Path) -> Result<Restored, InstallError>`, `holds(&self, file: ReplacedFile) -> bool`
  - `pub enum Restored { FromBackup, NothingToRestore }`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// The whole safety property: the *stock* engine is what gets saved. A second
    /// Deployment must not copy LuaJIT over the backup, or uninstall would "restore" the
    /// very thing it is meant to remove.
    #[test]
    fn a_backup_is_taken_once_and_never_overwritten() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let store = BackupStore::new(dir.path().join("backups"));
        let game_file = dir.path().join("lua51_Win32.dll");

        std::fs::write(&game_file, b"stock lua 5.1").expect("write");
        store
            .back_up_once(ReplacedFile::LuaEngine, &game_file)
            .expect("first backup");

        std::fs::write(&game_file, b"luajit").expect("overwrite as a Deployment would");
        store
            .back_up_once(ReplacedFile::LuaEngine, &game_file)
            .expect("second backup is a no-op");

        store
            .restore(ReplacedFile::LuaEngine, &game_file)
            .expect("restore");
        assert_eq!(
            std::fs::read(&game_file).expect("read"),
            b"stock lua 5.1",
            "the stock engine must come back, not the replacement"
        );
    }

    /// A player who cleared the App Data Store between install and uninstall has no backup.
    /// That is a thing to report, not a failure — uninstall still removes everything else.
    #[test]
    fn restoring_without_a_backup_says_so_instead_of_failing() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let store = BackupStore::new(dir.path().join("backups"));
        let game_file = dir.path().join("lua51_Win32.dll");
        std::fs::write(&game_file, b"luajit").expect("write");

        let outcome = store
            .restore(ReplacedFile::LuaEngine, &game_file)
            .expect("restore must not fail");
        assert_eq!(outcome, Restored::NothingToRestore);
    }
}
```

- [ ] **Step 2: Run and watch them fail**

Run: `cargo test -p civ5vp-core replaced`
Expected: FAIL — module not declared.

- [ ] **Step 3: Implement**

```rust
//! The Replaced File: a game-owned file the installer may overwrite, and the copy that
//! makes that reversible.
//!
//! ADR-0006. Everything here exists to keep one promise — a player who uninstalls gets back
//! the file the game shipped with.

use std::path::{Path, PathBuf};

use crate::claimed::GameFolders;
use crate::error::InstallError;
use crate::tree;

/// A file belonging to the game that the installer replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplacedFile {
    /// The Lua engine, in the Game Installation root beside the executables.
    LuaEngine,
}

impl ReplacedFile {
    pub fn file_name(self) -> &'static str {
        match self {
            Self::LuaEngine => "lua51_Win32.dll",
        }
    }

    /// Where it lives in the user's game.
    pub fn path_in(self, folders: &GameFolders) -> PathBuf {
        match self {
            Self::LuaEngine => folders.game_root.join(self.file_name()),
        }
    }
}

/// What a restore did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restored {
    FromBackup,
    /// No backup was held — nothing was changed.
    NothingToRestore,
}

/// Where the originals are kept, inside the App Data Store.
pub struct BackupStore {
    root: PathBuf,
}

impl BackupStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn path_of(&self, file: ReplacedFile) -> PathBuf {
        self.root.join(file.file_name())
    }

    /// Whether an original is already held.
    pub fn holds(&self, file: ReplacedFile) -> bool {
        self.path_of(file).is_file()
    }

    /// Save the game's copy — but only the first time.
    ///
    /// The guard is the entire point: by the second Deployment the file in the game is the
    /// installer's own replacement, and saving that would destroy the only copy of the
    /// original.
    pub fn back_up_once(&self, file: ReplacedFile, from: &Path) -> Result<(), InstallError> {
        if self.holds(file) || !from.is_file() {
            return Ok(());
        }
        tree::create_dir_all(&self.root)?;
        tree::copy_file(from, &self.path_of(file))
    }

    /// Put the original back, if one is held.
    pub fn restore(&self, file: ReplacedFile, to: &Path) -> Result<Restored, InstallError> {
        if !self.holds(file) {
            return Ok(Restored::NothingToRestore);
        }
        tree::copy_file(&self.path_of(file), to)?;
        Ok(Restored::FromBackup)
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p civ5vp-core replaced`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "core: Replaced Files, backed up once and restored on uninstall"
```

---

### Task 8: Deploying and uninstalling the engine

**Files:**
- Modify: `crates/core/src/install.rs:111-151` (`execute`), `:328` (`sync`), `:480` (`uninstall`)
- Modify: `crates/core/src/plan.rs` (expose the engine choice on the `Plan`)
- Test: `crates/core/tests/` (the existing install integration tests)

**Interfaces:**
- Consumes: `LuaJitEngine` (Task 3), `SourceProvider::materialize_luajit` (Task 4), `ToolchainRunner::build_luajit` + `LuaJitBuildRequest` (Task 6), `BackupStore`/`ReplacedFile`/`Restored` (Task 7).
- Produces: `InstallOutcome` gains `pub luajit_deployed: bool`; `UninstallOutcome` gains `pub engine_restored: Restored`.

- [ ] **Step 1: Write the failing tests**

Add to the install integration tests:

```rust
/// The game is not touched until everything that can fail has succeeded — including the
/// LuaJIT build. A build that fails must leave the stock engine exactly where it was.
#[test]
fn a_failing_luajit_build_leaves_the_game_engine_alone() {
    let world = World::new();
    world.write_game_file("lua51_Win32.dll", b"stock lua 5.1");
    let core = world.core_with_failing_luajit_build();
    let mut configuration = world.configuration();
    configuration.luajit = LuaJitEngine::LuaJit;

    let plan = core.plan(&configuration, &world.folders()).expect("a plan");
    let outcome = core.execute(&plan, &ProgressReporter::silent());

    assert!(outcome.is_err(), "the Deployment must fail");
    assert_eq!(
        world.read_game_file("lua51_Win32.dll"),
        b"stock lua 5.1",
        "the engine must be untouched"
    );
}

/// Opting in replaces the engine and keeps the original safe.
#[test]
fn opting_in_replaces_the_engine_and_backs_up_the_original() {
    let world = World::new();
    world.write_game_file("lua51_Win32.dll", b"stock lua 5.1");
    let core = world.core();
    let mut configuration = world.configuration();
    configuration.luajit = LuaJitEngine::LuaJit;

    let plan = core.plan(&configuration, &world.folders()).expect("a plan");
    let outcome = core.execute(&plan, &ProgressReporter::silent()).expect("deploy");

    assert!(outcome.luajit_deployed);
    assert_ne!(world.read_game_file("lua51_Win32.dll"), b"stock lua 5.1");

    let uninstalled = core
        .uninstall(&world.folders(), &ProgressReporter::silent())
        .expect("uninstall");
    assert_eq!(uninstalled.engine_restored, Restored::FromBackup);
    assert_eq!(world.read_game_file("lua51_Win32.dll"), b"stock lua 5.1");
}

/// Not opting in must change nothing about the engine, even on a machine that once had it.
#[test]
fn the_stock_engine_is_left_alone_when_luajit_is_not_chosen() {
    let world = World::new();
    world.write_game_file("lua51_Win32.dll", b"stock lua 5.1");
    let core = world.core();
    let configuration = world.configuration();

    let plan = core.plan(&configuration, &world.folders()).expect("a plan");
    core.execute(&plan, &ProgressReporter::silent()).expect("deploy");

    assert_eq!(world.read_game_file("lua51_Win32.dll"), b"stock lua 5.1");
}
```

Add `write_game_file` / `read_game_file` helpers to the test `World` if absent, writing into the fixture's `game_root`.

- [ ] **Step 2: Run and watch them fail**

Run: `cargo test -p civ5vp-core luajit`
Expected: FAIL — `luajit_deployed` not a field.

- [ ] **Step 3: Build LuaJIT before Sync**

In `Core::execute`, after the VP DLL is settled and before `sync`, add:

```rust
        // Built before Sync for the same reason the VP DLL is: the game is not touched
        // until everything that can fail has succeeded. A LuaJIT build that fails must
        // leave the player's engine exactly where it was.
        let built_luajit = if plan.luajit() {
            let source = self.source_provider.materialize_luajit(progress)?;
            let output_path = self.work_dir.join("build").join(LUA_ENGINE_FILE_NAME);
            self.toolchain_runner.build_luajit(
                &LuaJitBuildRequest {
                    source_root: source,
                    output_path: output_path.clone(),
                },
                progress,
            )?;
            Some(output_path)
        } else {
            None
        };
```

Thread `built_luajit.as_deref()` into `sync`.

- [ ] **Step 4: Replace the engine in Sync**

At the end of `sync`, after the Claimed work and before the cache is cleared:

```rust
        if let Some(built) = built_luajit {
            let destination = ReplacedFile::LuaEngine.path_in(&plan.folders);
            // Once, and only from the game's own copy — after the first Deployment the file
            // sitting there is ours, and saving it would lose the original for good.
            self.backups()
                .back_up_once(ReplacedFile::LuaEngine, &destination)?;
            tree::copy_file(built, &destination)?;
            progress.report(
                Stage::Sync,
                "Installed the LuaJIT engine. Your original was saved.",
            );
        }
```

Add `fn backups(&self) -> BackupStore { BackupStore::new(self.work_dir.join("backups")) }` and `const LUA_ENGINE_FILE_NAME: &str = "lua51_Win32.dll";`.

- [ ] **Step 5: Restore on uninstall**

In `Core::uninstall`, after the Claimed removals:

```rust
        // Unconditional: the player may be uninstalling a configuration that had LuaJIT even
        // though nothing in this call says so, and leaving a replaced engine behind would
        // not be "an unmodded game".
        let engine_restored = self
            .backups()
            .restore(ReplacedFile::LuaEngine, &ReplacedFile::LuaEngine.path_in(folders))?;
        if engine_restored == Restored::FromBackup {
            progress.report(Stage::Sync, "Restored the game's original Lua engine.");
        }
```

Return it on `UninstallOutcome`.

- [ ] **Step 6: Run tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "core: deploy the LuaJIT engine and restore the original on uninstall"
```

---

### Task 9: The checkbox

**Files:**
- Modify: `crates/installer/src/app.rs:120-164` (state), `:515-596` (the "What to install" panel), `:1150` (`configuration()`)
- Test: `crates/installer/tests/shell.rs`

**Interfaces:**
- Consumes: `LuaJitEngine` (Task 3); `InstallOutcome::luajit_deployed` (Task 8).
- Produces: `InstallerApp::luajit: bool`.

- [ ] **Step 1: Write the failing test**

```rust
/// The engine choice reaches the Core, and the default leaves the game's engine alone.
#[test]
fn the_luajit_checkbox_reaches_the_configuration() {
    let mut harness = shell_harness();
    let Some(configuration) = harness.app().configuration() else {
        unreachable!("a configuration is available once folders are settled")
    };
    assert_eq!(
        configuration.luajit,
        LuaJitEngine::Stock,
        "replacing a game file must never be the default"
    );

    harness.app_mut().luajit = true;
    let Some(configuration) = harness.app().configuration() else {
        unreachable!("a configuration is available once folders are settled")
    };
    assert_eq!(configuration.luajit, LuaJitEngine::LuaJit);
}
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test -p civ5vp-installer luajit_checkbox`
Expected: FAIL — no field `luajit`.

- [ ] **Step 3: Add the state and the checkbox**

Add `pub luajit: bool` to `InstallerApp`, defaulting to `false` and restored from settings.

In the "What to install" panel, beside the 43 Civs checkbox:

```rust
                let engine = ui
                    .checkbox(&mut self.luajit, "Use the LuaJIT engine")
                    .on_hover_text(
                        "Replaces the game's Lua engine with LuaJIT. Map generation and the \
                         interface get faster; AI turn times are decided by the mod's C++ code \
                         and will not change. Your original file is saved and put back if you \
                         uninstall. Some older Lua mods do not work with it.",
                    );
                if engine.changed() {
                    chosen = true;
                }
```

The hover text matters: ADR-0006 requires the UI not to promise faster turns.

- [ ] **Step 4: Map it in `configuration()`**

```rust
            luajit: if self.luajit {
                LuaJitEngine::LuaJit
            } else {
                LuaJitEngine::Stock
            },
```

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 6: Refresh the screenshots**

Run: `cargo test -p civ5vp-installer -- --ignored snapshot`
Review the changed baselines, then accept them per the repo's existing snapshot workflow.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "installer: an opt-in LuaJIT checkbox that does not overpromise"
```

---

### Task 10: The export-compatibility guard

The one failure mode that silently bricks the game is a DLL missing a symbol the game imports. That is checkable, so it is checked.

**Files:**
- Create: `crates/toolchain/src/luajit/exports.rs`
- Modify: `crates/toolchain/src/luajit/mod.rs`
- Test: `crates/toolchain/src/luajit/exports.rs` tests module

**Interfaces:**
- Consumes: nothing from earlier tasks; reads PE files directly.
- Produces: `pub fn exported_names(dll: &[u8]) -> Option<Vec<String>>`, `pub fn imported_lua_names(binary: &[u8]) -> Option<Vec<String>>`, and `pub fn missing_for(dll: &[u8], consumers: &[&[u8]]) -> Option<Vec<String>>`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// The stock engine trivially satisfies the game — which is what proves the checker
    /// itself is right before it is trusted to judge our own build.
    #[test]
    #[ignore = "needs a real Civilization V installation"]
    fn the_stock_engine_satisfies_the_game() {
        let Some(game) = game_directory_from_env() else {
            return;
        };
        let dll = std::fs::read(game.join("lua51_Win32.dll")).expect("the engine");
        let exe = std::fs::read(game.join("CivilizationV_DX11.exe")).expect("the game");
        let Some(missing) = missing_for(&dll, &[&exe]) else {
            unreachable!("both files parse as PE")
        };
        assert!(missing.is_empty(), "stock must satisfy stock: {missing:?}");
    }

    /// A DLL that exports nothing must be reported as missing everything — the checker has
    /// to actually fail, or it would pass a broken build too.
    #[test]
    fn a_dll_exporting_nothing_is_reported_as_missing_everything() {
        let empty = exports_fixture(&[]);
        let consumer = imports_fixture(&["lua_pcall", "lua_gettop"]);
        let Some(missing) = missing_for(&empty, &[&consumer]) else {
            unreachable!("the fixtures parse as PE")
        };
        assert_eq!(missing, vec!["lua_gettop".to_owned(), "lua_pcall".to_owned()]);
    }
}
```

Build `exports_fixture` / `imports_fixture` as minimal in-memory PE images with just the headers, a section table and the relevant data directory — enough for the parser, no linker involved.

- [ ] **Step 2: Run and watch it fail**

Run: `cargo test -p civ5vp-toolchain exports`
Expected: FAIL — module not declared.

- [ ] **Step 3: Implement the PE reader**

Parse: `e_lfanew` at `0x3c`; `Machine` at `+4`; `NumberOfSections` at `+6`; `SizeOfOptionalHeader` at `+20`; optional-header magic at `+24`; data directories at optional-header start `+96` (PE32). Directory 0 is exports, directory 1 is imports. Map RVAs through the section table. For exports, walk `NumberOfNames`/`AddressOfNames`. For imports, walk descriptors and follow `OriginalFirstThunk`, skipping ordinals (high bit set) and reading names at `RVA + 2`.

`missing_for` returns the sorted set difference: everything the consumers import from a DLL whose name contains `lua`, minus everything the candidate exports.

- [ ] **Step 4: Gate the build on it**

In `BootstrappedToolchain::build_luajit`, after linking and before copying to `output_path`, read the game's binaries and refuse a DLL that would not satisfy them:

```rust
        // A missing export is the one failure that would leave the player with a game that
        // cannot start, and it is entirely checkable — so it is checked, every build.
        if let Some(missing) = luajit::exports::missing_for(&built, &consumers)
            && !missing.is_empty()
        {
            return Err(BoundaryError::new(
                "The LuaJIT engine the installer built is missing functions the game needs, \
                 so it was not installed. Your game was not changed.",
                format!("missing exports: {}", missing.join(", ")),
            ));
        }
```

This needs the game root, so add `pub game_root: PathBuf` to `LuaJitBuildRequest` and populate it from `plan.folders.game_root` in Task 8's `execute`.

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "toolchain: refuse a LuaJIT build that would not satisfy the game"
```

---

### Task 11: End to end, against the real game

**Files:**
- Modify: the `#[ignore]`d real-install test alongside the existing one

- [ ] **Step 1: Write the test**

```rust
/// The whole path, against a real Civilization V and a real network: fetch, build, replace,
/// then put it back. Ignored by default — it downloads and compiles.
#[test]
#[ignore = "downloads and compiles; needs a real Civilization V installation"]
fn luajit_is_built_deployed_and_restored() {
    let Some(world) = RealWorld::from_env() else {
        return;
    };
    let mut configuration = world.configuration();
    configuration.luajit = LuaJitEngine::LuaJit;

    let engine = world.folders().game_root.join("lua51_Win32.dll");
    let before = std::fs::read(&engine).expect("the stock engine");
    assert!(
        String::from_utf8_lossy(&before).contains("Lua 5.1"),
        "this test must start from the stock engine"
    );

    let core = world.core();
    let plan = core.plan(&configuration, &world.folders()).expect("a plan");
    core.execute(&plan, &ProgressReporter::silent()).expect("deploy");

    let after = std::fs::read(&engine).expect("the new engine");
    assert!(
        String::from_utf8_lossy(&after).contains("LuaJIT"),
        "the engine must now be LuaJIT"
    );

    core.uninstall(&world.folders(), &ProgressReporter::silent())
        .expect("uninstall");
    assert_eq!(
        std::fs::read(&engine).expect("the restored engine"),
        before,
        "uninstall must give the player their file back, byte for byte"
    );
}
```

- [ ] **Step 2: Run it**

Run: `cargo test --workspace -- --ignored luajit_is_built_deployed_and_restored`
Expected: PASS.

- [ ] **Step 3: Launch the game once, by hand**

Start Civilization V with Vox Populi active and confirm it reaches the main menu and starts a game. This is the only check that the engine actually runs; nothing automated covers it.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "test: end-to-end LuaJIT deployment and restore"
```

---

## Self-Review

**Spec coverage.** User story 28 (opt in, get it built and deployed, get the original back on uninstall) is covered by Tasks 3, 8, 9 and 11. The ADR's three Replaced-File rules map to: backup-once (Task 7 Step 1 test), restore-on-uninstall (Task 8 Step 5), opt-in (Task 3's `#[default] Stock` plus Task 9's default-false checkbox). The "must not promise faster turns" consequence is enforced by the hover text in Task 9 Step 3. The Steam-verify consequence is handled implicitly — Sync copies the built DLL every Deployment, so a stock DLL restored by Steam is replaced again on the next run.

**Placeholder scan.** Two steps delegate to an existing pattern rather than repeating code: Task 4 Step 4 (`gix` shallow fetch, pointing at `crates/sources/src/upstream.rs`) and Task 6 Step 3's `lj_*.c` expansion. Both name the exact file to copy from and the exact behaviour required, which is the intent — the `gix` fetch options in this workspace are long and version-specific, and transcribing them from memory into a plan is how they get subtly wrong.

**Type consistency.** `LuaJitEngine` (not `LuaJit`) throughout Tasks 3, 8, 9, 11. `ReplacedFile::LuaEngine` throughout 7 and 8. `Restored::{FromBackup, NothingToRestore}` in 7 and 8. `LuaJitBuildRequest` gains `game_root` in Task 10 Step 4 — Task 6 Step 3 defines it with two fields and Task 10 adds the third, which is deliberate and flagged in both places.

**One gap worth naming.** `HostRunner::for_this_host` takes `steam_roots`, but `ToolchainRunner::build_luajit` is not given them — `BootstrappedToolchain` is constructed in `crates/installer/src/wiring.rs` with only a cache path. Task 5 must therefore also add `SearchLocations::for_this_platform().steam_roots` to `BootstrappedToolchain::new`, or resolve them inside the toolchain crate. Resolve this when implementing Task 5; it is a wiring change, not a design one.
