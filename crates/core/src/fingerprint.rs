//! The Build Fingerprint: everything that determines the Built DLL, in one comparable value.
//!
//! `CONTEXT.md`: "a hash of everything that determines the Built DLL: all source inputs at
//! the selected Version (or the Local Repo's working files), compiler flags, the 43-Civs
//! setting, and the Toolchain version. Recorded next to the deployed DLL together with the
//! DLL's own hash; when both still match, the build is skipped."
//!
//! The source-input half arrives from the source-provider boundary as
//! [`crate::MaterializedSource::source_identity`] - a git commit id for a checked-out
//! Version, a content walk over [`DLL_SOURCE_INPUT_ROOTS`] for a Local Repo. The compiler
//! flags are not hashed literally: every flag the toolchain runner passes is a function of
//! the Build Configuration, the 43-Civs toggle, the toolchain identity, and the sources
//! themselves (which carry the project file), so those four stand in for them. If a flag
//! ever stops being derivable from these, it must join the fingerprint explicitly - which is
//! exactly what the maintainer's optimisation override is, and why
//! [`crate::ToolchainRunner::dll_flag_override`] is one of the inputs below.
//!
//! Hashing is FNV-1a 64 implemented here in a dozen lines, because the Core has no
//! dependencies and the job is integrity against accident - a swapped DLL, an
//! edited source - not against an adversary forging collisions on purpose.

use std::fmt::Write as _;
use std::fs;
use std::io::Read as _;
use std::path::Path;

use crate::configuration::{BuildConfiguration, FortyThreeCivs};

/// The sidecar's file name, beside the deployed DLL inside `(1) Community Patch`.
pub const FINGERPRINT_FILE_NAME: &str = "CvGameCore_Expansion2.dll.fingerprint";

/// The directories (and one file) that hold every input the DLL build reads, relative to an
/// Installation Source root.
///
/// Used by the Local Repo provider to derive `source_identity` from working-file contents.
/// Deliberately the top-level compile roots rather than the exact per-Version file list: a
/// change to any file under them forces a rebuild even if the build happens not to read it -
/// conservative, so there are no false skips - while edits to the mod-content folders
/// (`(1)`, `(2)`, LUA, SQL…) never force a needless compile.
pub const DLL_SOURCE_INPUT_ROOTS: [&str; 8] = [
    "CvGameCoreDLL_Expansion2",
    "CvWorldBuilderMap",
    "CvGameCoreDLLUtil",
    "CvLocalization",
    "CvGameDatabase",
    "FirePlace",
    "ThirdPartyLibs",
    "clang.cpp",
];

/// Where the DLL a Deployment installed actually came from.
///
/// Part of the fingerprint rather than a note beside it, because it changes what the recorded
/// hash means: the same sources at the same Version give one DLL when compiled here and a
/// different one when taken from the repository (upstream's compiler is not ours). Without
/// this line, ticking "Compile the DLL myself" on an already-installed Release would find a
/// matching sidecar and skip the very build it asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DllProvenance {
    /// Compiled here, by the bootstrapped Toolchain.
    Built,
    /// The Shipped DLL, taken from the Installation Source as-is.
    Shipped,
}

impl DllProvenance {
    fn token(self) -> &'static str {
        match self {
            Self::Built => "built",
            Self::Shipped => "shipped",
        }
    }
}

/// A computed Build Fingerprint, ready to compare against a sidecar or be recorded in one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildFingerprint {
    rendered: String,
}

impl BuildFingerprint {
    /// Combine everything that determines the Built DLL.
    pub fn new(
        source_identity: &str,
        version_label: &str,
        configuration: BuildConfiguration,
        forty_three_civs: FortyThreeCivs,
        provenance: DllProvenance,
        toolchain_identity: &str,
        flag_override: Option<&str>,
    ) -> Self {
        let configuration = configuration.token();
        let provenance = provenance.token();
        let forty_three = match forty_three_civs {
            FortyThreeCivs::Enabled => "on",
            FortyThreeCivs::Disabled => "off",
        };
        // One line per input, in a fixed order - same input must yield the same
        // fingerprint, byte for byte. `v2` is the format's own version: a future installer whose fingerprint
        // means something different must not skip on a sidecar it does not understand. It
        // went to v2 when the `dll` line arrived, so a sidecar written before Shipped DLLs
        // existed - which always described a compiled one - is not read as either. The
        // installer version rides along because the compiler flags are *derived by this
        // code* from the other lines - a release that changes the derivation must
        // invalidate old sidecars, and a version bump is the one thing a release reliably
        // does. The cost is one rebuild per installer upgrade, which is also the honest
        // thing to do. It went to v3 when the `flags` line arrived: a v2 sidecar cannot say
        // whether the DLL beside it was built with an optimisation override, so it must not
        // be trusted to answer that question either way.
        let installer = env!("CARGO_PKG_VERSION");
        // Always written, `none` included, so that "no override" is a stated fact rather than
        // an absent line. A sidecar left behind by a run that *did* override must not read as
        // a default build.
        let flags = flag_override.unwrap_or("none");
        let rendered = format!(
            "fingerprint v3\ninstaller {installer}\nsource {source_identity}\n\
             label {version_label}\nconfiguration {configuration}\n\
             forty-three-civs {forty_three}\ndll {provenance}\n\
             toolchain {toolchain_identity}\nflags {flags}\n"
        );
        Self { rendered }
    }

    /// The sidecar contents: the fingerprint plus the hash of the DLL it produced.
    pub fn sidecar_contents(&self, built_dll_bytes_hash: u64) -> String {
        format!("{}dll fnv1a64:{built_dll_bytes_hash:016x}\n", self.rendered)
    }

    /// Whether `sidecar` records this same fingerprint, and if so, the DLL hash it promised.
    ///
    /// `None` means the sidecar is missing, unreadable, from another fingerprint version, or
    /// simply different - all of which mean the same thing to the caller: build.
    pub fn matches_sidecar(&self, sidecar: &str) -> Option<u64> {
        let (fingerprint_part, dll_line) = sidecar.split_once("dll fnv1a64:")?;
        if fingerprint_part != self.rendered {
            return None;
        }
        u64::from_str_radix(dll_line.trim(), 16).ok()
    }
}

/// FNV-1a, 64-bit, incremental - the one hash everything here uses.
struct Fnv1a(u64);

impl Fnv1a {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn update(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

/// FNV-1a, 64-bit, of one buffer.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = Fnv1a::new();
    hash.update(bytes);
    hash.0
}

/// Hash a file's contents without reading it into memory at once (the DLL is ~10 MB).
/// `None` for any IO failure - to every caller that means "cannot be trusted: rebuild".
pub fn fnv1a64_of_file(path: &Path) -> Option<u64> {
    let mut file = fs::File::open(path).ok()?;
    let mut hash = Fnv1a::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            return Some(hash.0);
        }
        hash.update(&buffer[..read]);
    }
}

/// Derive a `source_identity` from the working files under `root` - the Local Repo case,
/// where there is no commit to name.
///
/// Walks [`DLL_SOURCE_INPUT_ROOTS`] in a fixed order, hashing each file's relative path and
/// contents. Roots that do not exist contribute their absence, so adding one later changes
/// the identity. Returns `Err` with the offending path when a file cannot be read - an
/// unreadable input must fail loudly rather than fingerprint as "unchanged".
pub fn dll_source_identity(root: &Path) -> Result<String, std::path::PathBuf> {
    let mut combined: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |value: u64| {
        for byte in value.to_le_bytes() {
            combined ^= u64::from(byte);
            combined = combined.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for input_root in DLL_SOURCE_INPUT_ROOTS {
        let path = root.join(input_root);
        mix(fnv1a64(input_root.as_bytes()));
        if path.is_file() {
            mix(fnv1a64_of_file(&path).ok_or_else(|| path.clone())?);
        } else if path.is_dir() {
            hash_directory(&path, &path, &mut mix)?;
        } else {
            mix(0);
        }
    }
    let mut rendered = String::from("files fnv1a64:");
    let _ = write!(rendered, "{combined:016x}");
    Ok(rendered)
}

fn hash_directory(
    directory: &Path,
    relative_to: &Path,
    mix: &mut impl FnMut(u64),
) -> Result<(), std::path::PathBuf> {
    let entries = fs::read_dir(directory).map_err(|_| directory.to_path_buf())?;
    // Sorted, so the identity does not depend on readdir order.
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    for path in paths {
        let relative = path.strip_prefix(relative_to).unwrap_or(&path);
        // The OS bytes, not a lossy string: two names that differ only outside UTF-8 must
        // not hash alike - a false skip is exactly what that would risk.
        mix(fnv1a64(relative.as_os_str().as_encoded_bytes()));
        if path.is_dir() {
            hash_directory(&path, relative_to, mix)?;
        } else {
            mix(fnv1a64_of_file(&path).ok_or_else(|| path.clone())?);
        }
    }
    Ok(())
}
