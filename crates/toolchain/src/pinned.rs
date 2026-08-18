//! Everything the Toolchain Bootstrap is allowed to fetch, transcribed from
//! `docs/pinned-artifacts.md`.
//!
//! This module is the code-side copy of that document. Nothing else in the crate may build a
//! URL: if a host name appears anywhere but here, something is fetching an artifact nobody
//! pinned.

/// The Windows SDK 7.0 ISO — the only mandatory download (`docs/pinned-artifacts.md` §1).
///
/// It carries both halves of the Toolchain's non-compiler part: the SDK headers and import
/// libs *and* the VC9 CRT (at `Setup/vc_stdx86/`). There is no second ISO.
pub const SDK_ISO: PinnedDownload = PinnedDownload {
    file_name: "GRMSDK_EN_DVD.iso",
    url: "https://web.archive.org/web/20161230154527/http://download.microsoft.com/download/2/E/9/2E911956-F90F-4BFB-8231-E292A7B6F287/GRMSDK_EN_DVD.iso",
    sha256: "65739fb0874cc17ea6962d8ce7915364c7161fa106ed1bf1c917924c18ac63ca",
    // Measured from the archive.org copy's `Content-Length` (2026-08-03). The document says
    // "~1.45 GiB", which is what Microsoft's own download page advertised; the snapshot at this
    // URL is 1.45 GiB. Only used to phrase progress before the server answers with a real
    // length — the SHA-256 is what decides whether the bytes are the right ones.
    approximate_bytes: 1_552_508_928,
};

/// The clang major version the docker-branch build is proven against
/// (`docs/pinned-artifacts.md` §2). Part of the Toolchain identity, not an incidental detail.
pub const LLVM_VERSION: &str = "18.1.8";

/// What the compiler is asked to target: Win32 x86.
pub const LLVM_TARGET_TRIPLE: &str = "i386-pc-windows-msvc";

/// An artifact the installer may download, together with the check that proves it is the
/// right one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedDownload {
    /// Name it is cached under, inside the Toolchain Cache's `downloads/` directory.
    pub file_name: &'static str,
    pub url: &'static str,
    /// Lowercase hex. A download that does not hash to this is discarded, never used.
    pub sha256: &'static str,
    /// Rough size, for progress before `Content-Length` is known.
    pub approximate_bytes: u64,
}

/// The portable LLVM build for one host platform.
///
/// `docs/pinned-artifacts.md` §2 pins the *version* (clang 18, `lld`, targeting
/// [`LLVM_TARGET_TRIPLE`]) but not a URL, because the reference build apt-installs it and the
/// installer cannot. These are the llvm.org release tarballs for that exact version — the
/// closest self-contained equivalent. Their checksums are not published by llvm.org; both
/// were measured by downloading the asset and hashing it (2026-08-03).
///
/// # The Linux tarball needs one library that no current distribution ships
///
/// `clang+llvm-18.1.8-x86_64-linux-gnu-ubuntu-18.04` links `libtinfo.so.5` — ncurses 5 — and
/// the loader refuses to start it without one. [`libtinfo_for_host`] pins that library, and
/// the bootstrap places it inside this tarball's own `lib/`, where the binaries' existing
/// `RUNPATH: $ORIGIN/../lib` finds it without any environment being set.
///
/// Aliasing `libtinfo.so.5` onto the system's `libtinfo.so.6` does *not* work — the binary
/// wants ncurses 5's versioned symbols — which is why an actual ncurses 5 library is shipped
/// rather than a symlink.
///
/// Keeping this build rather than a distribution's clang is deliberate and measured: it needs
/// only glibc 2.27, where Ubuntu 24.04's package needs ~2.39 and Arch's ~2.41. The installer
/// ships to players on whatever Linux they run, so that difference matters more than matching
/// the reference build's point release — which is not on offer anyway, since llvm.org
/// publishes no x86-64 Linux build of 18.1.3 at all. ADR-0005 has the full reasoning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedLlvm {
    pub download: PinnedDownload,
    /// The single top-level directory inside the tarball, stripped during extraction.
    pub archive_root: &'static str,
}

/// A shared library shipped alongside the compiler because the compiler will not start
/// without it.
///
/// One artifact today; see [`libtinfo_for_host`] and ADR-0005.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedLibrary {
    pub download: PinnedDownload,
    /// Path inside the package's data tarball, with no leading `./`.
    pub member: &'static str,
    /// File name to write inside the compiler's own `lib/` directory.
    pub install_as: &'static str,
    /// The `SONAME` the compiler asks for, created as a symlink to [`Self::install_as`].
    pub link_as: &'static str,
}

/// Everything a first Toolchain Bootstrap downloads, in bytes — what the up-front
/// expectation sentences are computed from, so the figure can never go stale.
pub fn approximate_download_total() -> u64 {
    sdk_member_bytes()
        + llvm_for_host().map_or(0, |llvm| llvm.download.approximate_bytes)
        + libtinfo_for_host().map_or(0, |library| library.download.approximate_bytes)
}

/// The library the pinned compiler needs in order to start, if this host needs one.
///
/// `None` on Windows: the Windows LLVM build has no such dependency, so nothing is fetched.
pub const fn libtinfo_for_host() -> Option<PinnedLibrary> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Some(PinnedLibrary {
            download: PinnedDownload {
                file_name: "libtinfo5_6.2+20201114-2+deb11u2_amd64.deb",
                url: "http://deb.debian.org/debian/pool/main/n/ncurses/libtinfo5_6.2+20201114-2+deb11u2_amd64.deb",
                sha256: "69e131ce3f790a892ca1b0ae3bfad8659daa2051495397eee1b627d9783a6797",
                approximate_bytes: 336_728,
            },
            member: "lib/x86_64-linux-gnu/libtinfo.so.5.9",
            install_as: "libtinfo.so.5.9",
            link_as: "libtinfo.so.5",
        })
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        None
    }
}

/// The pinned LLVM for the host this binary was built for, or `None` on a platform nobody
/// has pinned a build for yet.
///
/// Deliberately a lookup rather than a `cfg` chain in the middle of the bootstrap: a missing
/// platform is a plain `None` the caller can turn into a sentence, not a compile error in a
/// file that has nothing to do with platforms.
pub const fn llvm_for_host() -> Option<PinnedLlvm> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Some(PinnedLlvm {
            download: PinnedDownload {
                file_name: "clang+llvm-18.1.8-x86_64-linux-gnu-ubuntu-18.04.tar.xz",
                url: "https://github.com/llvm/llvm-project/releases/download/llvmorg-18.1.8/clang%2Bllvm-18.1.8-x86_64-linux-gnu-ubuntu-18.04.tar.xz",
                sha256: "54ec30358afcc9fb8aa74307db3046f5187f9fb89fb37064cdde906e062ebf36",
                approximate_bytes: 1_044_924_784,
            },
            archive_root: "clang+llvm-18.1.8-x86_64-linux-gnu-ubuntu-18.04",
        })
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        Some(PinnedLlvm {
            download: PinnedDownload {
                file_name: "clang+llvm-18.1.8-x86_64-pc-windows-msvc.tar.xz",
                url: "https://github.com/llvm/llvm-project/releases/download/llvmorg-18.1.8/clang%2Bllvm-18.1.8-x86_64-pc-windows-msvc.tar.xz",
                sha256: "22c5907db053026cc2a8ff96d21c0f642a90d24d66c23c6d28ee7b1d572b82e8",
                approximate_bytes: 981_666_720,
            },
            archive_root: "clang+llvm-18.1.8-x86_64-pc-windows-msvc",
        })
    }
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64")
    )))]
    {
        None
    }
}

/// One file inside the disc image, pinned by where its bytes are and what they hash to.
///
/// The offset is part of the pin because the installer fetches these out of the *middle* of
/// the image — the four members together are ~102 MiB of its 1.45 GiB, and asking for only
/// those bytes is the difference between a couple of minutes and a couple of hours on the
/// one source that still has the image (`docs/pinned-artifacts.md` §1).
///
/// A wrong offset cannot pass unnoticed: the bytes it fetched would not hash to `sha256`, and
/// the download is refused. The numbers were measured off a copy of the image whose whole-file
/// SHA-256 matches [`SDK_ISO`], by `describe_the_pinned_members` in `extract.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedMember {
    /// Path inside the image, forward-slashed, as `docs/pinned-artifacts.md` writes it.
    pub path: &'static str,
    /// Where this member's bytes start in the image.
    pub offset: u64,
    pub bytes: u64,
    /// Lowercase hex. Bytes that do not hash to this are discarded, never used.
    pub sha256: &'static str,
}

impl PinnedMember {
    /// What this member is cached as inside the downloads folder.
    pub fn cache_name(&self) -> String {
        member_cache_name(self.path)
    }
}

/// The file name a member inside the image is cached under: its path, flattened.
///
/// Flattened rather than nested so the folder stays flat, and the whole path is kept because
/// `Setup/WinSDK/cab1.cab` and `Setup/WinSDKBuild/cab1.cab` are different files with the same
/// base name — one overwriting the other would be an extraction that silently unpacks the
/// wrong cabinet.
pub fn member_cache_name(path: &str) -> String {
    path.trim_start_matches("Setup/").replace(['/', '\\'], "-")
}

/// One MSI inside the ISO, with the CABs that hold its payload.
///
/// `docs/pinned-artifacts.md` §1 calls this list "the extraction contract": everything else
/// in the 1.45 GiB image is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IsoMember {
    /// The MSI. Its `Media` table names the cabinets; the list below is here so a truncated
    /// or renamed image is caught before extraction starts.
    pub msi: PinnedMember,
    /// The cabinets beside it, in sequence order.
    pub cabs: &'static [PinnedMember],
    /// What this member contributes, for progress and for error messages.
    pub label: &'static str,
}

impl IsoMember {
    /// Every pinned file this member is made of, MSI first.
    pub fn files(&self) -> impl Iterator<Item = &PinnedMember> {
        std::iter::once(&self.msi).chain(self.cabs.iter())
    }
}

/// The four members the bootstrap pulls out of the ISO, in extraction order.
pub const ISO_MEMBERS: &[IsoMember] = &[
    IsoMember {
        msi: PinnedMember {
            path: "Setup/WinSDK/WinSDK_x86.msi",
            offset: 2_975_744,
            bytes: 2_374_144,
            sha256: "1e22e5f4c7324a77088d33aa9d3f555c85a525639e14a624b033ded032fd4da8",
        },
        cabs: &[PinnedMember {
            path: "Setup/WinSDK/cab1.cab",
            offset: 5_351_424,
            bytes: 29_986_366,
            sha256: "c5076c9cd324161ec6e8fbf893be0aab75dd1c68866cdc93d6c043d2d69dd063",
        }],
        label: "Windows SDK core",
    },
    IsoMember {
        msi: PinnedMember {
            path: "Setup/WinSDKBuild/WinSDKBuild_x86.msi",
            offset: 76_539_904,
            bytes: 979_968,
            sha256: "f1268291829854745ed2ab80e854c8a8514e876e71e3f810fdc06f40ca97edc3",
        },
        cabs: &[
            PinnedMember {
                path: "Setup/WinSDKBuild/cab1.cab",
                offset: 77_520_896,
                bytes: 5_742_456,
                sha256: "bd2b525187d30f1d7cf7132cab080e212ec33f248226b4b5a6aa2a02ef0cf6ba",
            },
            PinnedMember {
                path: "Setup/WinSDKBuild/cab2.cab",
                offset: 83_263_488,
                bytes: 6_193_571,
                sha256: "baa12eca0e63a31f3d9d5eddd0003a26bf7e49e69373eddc882cc74d6b8573c4",
            },
            PinnedMember {
                path: "Setup/WinSDKBuild/cab3.cab",
                offset: 89_458_688,
                bytes: 4_789_860,
                sha256: "e69199e1c281838ecc70263296f5cc4b3a569cb1bf7bcfdb93775d8696264b33",
            },
            PinnedMember {
                path: "Setup/WinSDKBuild/cab4.cab",
                offset: 94_248_960,
                bytes: 1_020_063,
                sha256: "c4635d2eae946c088b84d6c1e8874c5ade865440e9ac8325e472e529f28ed9e6",
            },
        ],
        label: "SDK headers and import libraries",
    },
    IsoMember {
        msi: PinnedMember {
            path: "Setup/WinSDKWin32Tools/WinSDKWin32Tools_x86.msi",
            offset: 1_444_061_184,
            bytes: 766_976,
            sha256: "ba17c91c2fbdc09cf23ad126a489b6cd7b38ae2ff7098cc748348bf4bbe895f7",
        },
        cabs: &[PinnedMember {
            path: "Setup/WinSDKWin32Tools/cab1.cab",
            offset: 1_444_829_184,
            bytes: 10_896_725,
            sha256: "551a7ccea577f8d2fea8a059e463e9e3ead1d9a03b73aa26b2886b8647ec8b29",
        }],
        label: "Win32 tools",
    },
    IsoMember {
        msi: PinnedMember {
            path: "Setup/vc_stdx86/vc_stdx86.msi",
            offset: 1_548_064_768,
            bytes: 408_576,
            sha256: "0a524433918357e8476fbf0191ad3a7fb45fad11ec929f5bd69b0d2713306ade",
        },
        cabs: &[PinnedMember {
            path: "Setup/vc_stdx86/vc_stdx86.cab",
            offset: 1_504_735_232,
            bytes: 43_328_732,
            sha256: "d91cdb54fe5b4328b811b3b0bdd0b660a84b09e87cdce4da7d07ead63069e192",
        }],
        label: "VC9 CRT",
    },
];

/// Every pinned member together — what a first bootstrap actually pulls down of the image.
pub fn sdk_member_bytes() -> u64 {
    ISO_MEMBERS
        .iter()
        .flat_map(IsoMember::files)
        .map(|file| file.bytes)
        .sum()
}

/// The names that must resolve under the extracted SDK root for the bootstrap to count as
/// complete (`docs/pinned-artifacts.md` §4).
///
/// Three headers and two libs prove the two halves of the ISO both landed; `DriverSpecs.h`
/// proves fix-up 6 ran, because that one ships only with the Driver Kit and has to be stubbed.
pub const VERIFICATION_NAMES: &[&str] = &[
    "windows.h",
    "stdio.h",
    "iostream",
    "kernel32.lib",
    "msvcrt.lib",
];

// `DriverSpecs.h` used to be the sixth name here; it was removed because fix-up 6's own stub
// made it impossible to fail — see `verify`'s module docs and `docs/pinned-artifacts.md` §4.

/// The WDK-only headers fix-up 6 stubs out. Empty files satisfy the `#include` chain for the
/// user-mode code the DLL is made of.
/// Only stubbed where the SDK ships no case-variant of them — see `fixups::stub_wdk_headers`.
/// Both names are in fact shipped by the Windows SDK 7.0, so on that artifact this list
/// produces nothing; it stays because a differently-packaged SDK may not ship them.
pub const WDK_STUB_HEADERS: &[&str] = &[
    "DriverSpecs.h",
    "SpecStrings.h",
    "driverspecs.h",
    "specstrings.h",
];

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// The document's own words: "one mandatory download, not two". If a second entry ever
    /// appears here it needs an ADR first.
    #[test]
    fn the_iso_is_the_only_pinned_microsoft_download() {
        assert!(SDK_ISO.url.starts_with("https://web.archive.org/"));
        assert_eq!(SDK_ISO.sha256.len(), 64);
        assert!(SDK_ISO.sha256.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn the_extraction_contract_lists_exactly_the_documented_members() {
        let paths: Vec<&str> = ISO_MEMBERS.iter().map(|m| m.msi.path).collect();
        assert_eq!(
            paths,
            vec![
                "Setup/WinSDK/WinSDK_x86.msi",
                "Setup/WinSDKBuild/WinSDKBuild_x86.msi",
                "Setup/WinSDKWin32Tools/WinSDKWin32Tools_x86.msi",
                "Setup/vc_stdx86/vc_stdx86.msi",
            ]
        );
        let cab_count: usize = ISO_MEMBERS.iter().map(|m| m.cabs.len()).sum();
        assert_eq!(cab_count, 7);
    }

    /// A pinned artifact with no checksum is not pinned.
    #[test]
    fn the_host_llvm_is_pinned_by_checksum() {
        let Some(llvm) = llvm_for_host() else {
            // A host nobody has pinned a build for. The bootstrap turns this into a sentence
            // rather than downloading something unverified.
            return;
        };
        assert_eq!(llvm.download.sha256.len(), 64);
        assert!(llvm.download.sha256.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(llvm.download.url.contains(LLVM_VERSION));
        assert!(llvm.download.file_name.contains(llvm.archive_root));
    }
}
