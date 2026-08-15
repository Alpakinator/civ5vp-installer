# Ship `libtinfo.so.5` beside the portable LLVM, rather than swapping to a distro compiler

The pinned portable LLVM does not start on any current Linux distribution. It links
`libtinfo.so.5` — ncurses 5 — which nothing has shipped for years. The obvious fix is to swap
the pin for a distribution's own clang package, which is also what the reference build uses.

**We are not doing that.** We keep the llvm.org build and add one 336 KB pinned artifact: the
`libtinfo5` package, from which one shared library is extracted into the toolchain's own `lib/`.

## Why not a distribution package

A distribution build inherits that distribution's glibc floor, and the installer ships to players
on whatever Linux they happen to run. Measured with `objdump -T` on the actual binaries:

| build | glibc floor | consequence |
| --- | --- | --- |
| llvm.org, built on Ubuntu 18.04 | **2.27** | runs on essentially any Linux since 2018 |
| Ubuntu 24.04 `clang-18` package | ~2.39 | excludes Ubuntu 22.04, Debian 12, RHEL 9 |
| Arch `clang18` package | ~2.41 | rolling distributions only |

Vox Populi's Linux players are mostly on Ubuntu, Mint and SteamOS. Pinning Arch's package would
break the installer for nearly all of them; Ubuntu 24.04's would break it for anyone on the
previous LTS. The compiler we already pin is the most portable one available, and its only
defect is one library that has nothing to do with compiling.

`libtinfo` is ncurses' terminal-capability library. clang uses it to decide whether to colour
its diagnostics. The compiler is fully functional without it — it just refuses to *start*.

## Why the earlier attempt failed, and why this is different

Aliasing `libtinfo.so.5` onto the system's `libtinfo.so.6` does not work: ncurses 5 and 6 export
incompatible versioned symbols, so the loader rejects it. That is a real dead end, and it is what
made this look unfixable.

Shipping the genuine ncurses-5 library is a different thing, and it works. Verified:

```
without:  clang-18: error while loading shared libraries: libtinfo.so.5
with:     clang version 18.1.8          lld-link: LLD 18.1.8
```

**No `LD_LIBRARY_PATH` is required.** The llvm.org binaries already carry
`RUNPATH: $ORIGIN/../lib`, so a library placed in the tarball's own `lib/` is found with no
environment manipulation at all — which matters, because the toolchain runner would otherwise
have to fabricate an environment for every compiler invocation.

End to end with a clean environment, this compiles C++ including `<windows.h>` and `<string>`
through `clang-cl --target=i386-pc-windows-msvc` against the SDK the installer extracts itself,
producing an `Intel i386 COFF object file`.

## The artifact

Debian's package rather than Ubuntu's, for one concrete reason: Debian's `data.tar.xz` is
decodable with `lzma-rs`, which the toolchain crate already depends on, while Ubuntu's
`data.tar.zst` would add a compression crate for a single 336 KB download.

| | |
| --- | --- |
| **Package** | `libtinfo5_6.2+20201114-2+deb11u2_amd64.deb` |
| **URL** | `http://deb.debian.org/debian/pool/main/n/ncurses/libtinfo5_6.2+20201114-2+deb11u2_amd64.deb` |
| **Size** | 336,728 bytes |
| **SHA-256** | `69e131ce3f790a892ca1b0ae3bfad8659daa2051495397eee1b627d9783a6797` |
| **Member wanted** | `lib/x86_64-linux-gnu/libtinfo.so.5.9` (191,928 bytes) |
| **Installed as** | `<toolchain>/llvm-<version>/lib/libtinfo.so.5.9`, plus a `libtinfo.so.5` symlink |
| **Licence** | MIT/X11 (ncurses) — redistributable |

`snapshot.debian.org` archives 94 versions of this package permanently, so the pin has a stable
home even after the Debian release goes end-of-life — unlike a distribution pool, which rotates.

A `.deb` is an `ar` archive: a `!<arch>\n` magic followed by 60-byte plain-text headers. That is
a few dozen lines to parse and needs no dependency, which is the same call this project already
made for the CLI, the VDF parser and the settings format.

## Consequences

* First bootstrap grows by 336 KB, against artifacts already measuring gigabytes. Negligible.
* One more pinned URL, and a third archive host (`deb.debian.org`) alongside `web.archive.org`
  and `github.com`.
* No new crate dependency.
* **Windows is unaffected** — the Windows LLVM build has no such dependency, so this artifact is
  fetched only on Linux.
* We keep a compiler that is *not* the reference build's exact point release: llvm.org 18.1.8
  against Ubuntu 24.04's 18.1.3. Both are LLVM 18.1, and llvm.org publishes no Linux build of
  18.1.3 at all, so matching exactly is not on offer without taking a distribution package and
  its glibc floor. The spec's warning is about the clang *configuration* — flags, target, the
  Release settings — not about point releases, and the configuration is copied from the docker
  branch unchanged.

## Status

Decided and verified by hand; **not yet implemented in code**. What exists today is the pinned
LLVM that does not start. Implementing this means: add the artifact to `pinned.rs`, parse the
`ar` container, decompress `data.tar.xz` with the existing `lzma-rs`, and place the library with
its symlink. The test that would prove it is one that compiles a `windows.h` translation unit —
the same test that would have caught the fix-up bug recorded in ticket 05.
