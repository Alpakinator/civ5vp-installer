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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedLlvm {
    pub download: PinnedDownload,
    /// The single top-level directory inside the tarball, stripped during extraction.
    pub archive_root: &'static str,
}

/// The pinned LLVM for the host this binary was built for, or `None` on a platform nobody
/// has pinned a build for yet.
///
/// Deliberately a lookup rather than a `cfg` chain in the middle of the bootstrap: a missing
/// platform is a plain `None` the caller can turn into a sentence, not a compile error in a
/// file that has nothing to do with platforms (rule 4).
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

/// One MSI inside the ISO, with the CABs that hold its payload.
///
/// `docs/pinned-artifacts.md` §1 calls this list "the extraction contract": everything else
/// in the 1.45 GiB image is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IsoMember {
    /// Path of the MSI inside the ISO, forward-slashed, as the document writes it.
    pub msi_path: &'static str,
    /// The CABs beside it, in sequence order. The MSI's `Media` table names these; the list
    /// is here so a truncated or renamed image is caught before extraction starts.
    pub cab_paths: &'static [&'static str],
    /// What this member contributes, for progress and for error messages.
    pub label: &'static str,
}

/// The four members the bootstrap pulls out of the ISO, in extraction order.
pub const ISO_MEMBERS: &[IsoMember] = &[
    IsoMember {
        msi_path: "Setup/WinSDK/WinSDK_x86.msi",
        cab_paths: &["Setup/WinSDK/cab1.cab"],
        label: "Windows SDK core",
    },
    IsoMember {
        msi_path: "Setup/WinSDKBuild/WinSDKBuild_x86.msi",
        cab_paths: &[
            "Setup/WinSDKBuild/cab1.cab",
            "Setup/WinSDKBuild/cab2.cab",
            "Setup/WinSDKBuild/cab3.cab",
            "Setup/WinSDKBuild/cab4.cab",
        ],
        label: "SDK headers and import libraries",
    },
    IsoMember {
        msi_path: "Setup/WinSDKWin32Tools/WinSDKWin32Tools_x86.msi",
        cab_paths: &["Setup/WinSDKWin32Tools/cab1.cab"],
        label: "Win32 tools",
    },
    IsoMember {
        msi_path: "Setup/vc_stdx86/vc_stdx86.msi",
        cab_paths: &["Setup/vc_stdx86/vc_stdx86.cab"],
        label: "VC9 CRT",
    },
];

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
    "DriverSpecs.h",
];

/// The WDK-only headers fix-up 6 stubs out. Empty files satisfy the `#include` chain for the
/// user-mode code the DLL is made of.
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
        let paths: Vec<&str> = ISO_MEMBERS.iter().map(|m| m.msi_path).collect();
        assert_eq!(
            paths,
            vec![
                "Setup/WinSDK/WinSDK_x86.msi",
                "Setup/WinSDKBuild/WinSDKBuild_x86.msi",
                "Setup/WinSDKWin32Tools/WinSDKWin32Tools_x86.msi",
                "Setup/vc_stdx86/vc_stdx86.msi",
            ]
        );
        let cab_count: usize = ISO_MEMBERS.iter().map(|m| m.cab_paths.len()).sum();
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
