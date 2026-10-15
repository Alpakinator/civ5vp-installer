# A Local Repo's DLL is named by git, and that name beats the checkout's own `commit_id.inc`

The DLL compiles in a version string, `CURRENT_GAMECORE_VERSION`, from `commit_id.inc`. The
game writes it into its log, its savegames and every crash report (`crashes.log`'s
`DLL-Version` line and the minidump's file name). Upstream's build scripts generate the file
at the repository root with `git describe --tags --long` plus `Clean` or `Dirty`.

The installer used to read no git at all from a Local Repo (spec story 29: "zero git
operations performed on it"), so it named every Local Repo DLL `Local Installer`, and let a
`commit_id.inc` already in the checkout win, as part of using the tree "as-is".

**Both were wrong, and we changed them:**

- The installer now reads git from a Local Repo - `HEAD`, the tags, the index and the tracked
  files - to build the same name upstream's scripts build, for example
  `Release-5.4.7-3-gd4ff625ef Dirty Installer`. It still writes nothing into the checkout.
  Story 29's point was that the installer must never change the developer's tree, and a read
  does not.
- The installer's generated file wins over the checkout's own, through a clang VFS overlay
  that puts it at `<root>/commit_id.inc` (`crates/toolchain/src/build/mod.rs`).

## Why

On 2026-10-15 every crash from a Local Repo build since 2026-09-02 reported
`Release-5.4.6-1-gaa01f4026 Dirty`. That was a `commit_id.inc` written once by a scratch build
script, which nothing ever regenerated. Attributing those crashes took image sizes and link
timestamps from the minidumps, because the one field meant to answer "which code crashed" was
wrong. A plain `Local` would have been no better: it is true and says nothing.

Nothing in the checkout ever removes the file. Visual Studio's pre-build step and upstream's
scripts rewrite it on every build, and nothing else touches it. So "as-is" kept whatever the
last non-installer build wrote, and that is never the name of what the installer is
compiling.

## Consequences

- The name is compiled in, so it is part of the Build Fingerprint (`version-string`, format
  v4). Committing work turns `Dirty` into `Clean` and rebuilds even if no DLL source changed.
- A new name recompiles only the sources that include `commit_id.inc` (three of them), not
  the whole tree. It stays outside the PCH fence, which would rebuild everything.
- A tree git cannot read, such as an exported archive, still builds, under `Local`, and the
  progress log says why.
- The overlay is verified with the pinned clang 18 on Linux. Windows path handling in clang's
  overlay is unverified here: no Windows machine. If the overlay ever misses, the old
  include-directory fallback still supplies the generated file wherever the checkout has
  none of its own.
