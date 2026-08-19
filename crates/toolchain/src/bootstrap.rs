//! Toolchain Bootstrap: make the Toolchain exist, once.
//!
//! The sequence is fixed and every step is restartable from wherever the last one stopped:
//!
//! 1. Give back whatever an older version of the installer downloaded and this one does not
//!    need - see [`ToolchainBootstrap::reclaim_superseded_downloads`]. Before the next step,
//!    because a cache that is already complete is exactly the one still holding it.
//! 2. Is the Toolchain Cache complete? Then nothing else happens at all.
//! 3. Throw away any half-finished tree - a killed bootstrap must not leave a cache that
//!    looks usable.
//! 4. Download the eleven pinned members of the Windows SDK image and the portable LLVM,
//!    resuming and hash-checking each.
//! 5. Extract the four pinned members through MSI and CAB, in process.
//! 6. Check the extraction produced an `Include` and a `Lib` at all.
//! 7. Apply the six Linux fix-ups.
//! 8. Verify the six names from `docs/pinned-artifacts.md` §4 resolve.
//! 9. Only then write the completeness marker.

use std::fs;
use std::path::{Path, PathBuf};

use civ5vp_core::{ProgressReporter, Stage};

use crate::cache::{Toolchain, ToolchainCache};
use crate::download::{ByteSource, HttpByteSource, fetch};
use crate::error::{ToolchainError, io_error};
use crate::members;
use crate::pinned::{
    ISO_MEMBERS, IsoMember, PinnedDownload, PinnedLibrary, PinnedLlvm, SDK_ISO, libtinfo_for_host,
    llvm_for_host,
};
use crate::verify::{require_complete, verify_extraction};
use crate::{deb, extract, fixups, sdk_layout, tarball};

/// Where the members fetched out of the disc image are kept, inside the downloads folder.
///
/// Beside the whole-artifact downloads rather than among them: they are pieces of one
/// artifact, and a folder makes that obvious to anyone looking at the cache.
const SDK_MEMBERS_DIR: &str = "sdk";

/// Acquires the Toolchain into a [`ToolchainCache`].
///
/// Construct it with the cache root - inside the App Data Store, per `CONTEXT.md` - and call
/// [`ToolchainBootstrap::ensure`]. It is cheap to construct and safe to call repeatedly.
pub struct ToolchainBootstrap {
    cache: ToolchainCache,
    source: Box<dyn ByteSource + Send + Sync>,
    /// What to fetch. Fields rather than constants read inline, so the fast suite can point
    /// the *real* [`ToolchainBootstrap::ensure`] at artifacts it built itself instead of
    /// re-implementing the sequence in a test helper.
    sdk_iso: PinnedDownload,
    /// Which pieces of the image are fetched out of it, and where they are.
    sdk_members: &'static [IsoMember],
    llvm: Option<PinnedLlvm>,
    libtinfo: Option<PinnedLibrary>,
}

impl ToolchainBootstrap {
    /// The real bootstrap, downloading the pinned artifacts over HTTPS.
    pub fn new(cache_root: PathBuf) -> Self {
        Self::with_byte_source(cache_root, Box::new(HttpByteSource::new()))
    }

    /// The same bootstrap with the network replaced.
    pub fn with_byte_source(
        cache_root: PathBuf,
        source: Box<dyn ByteSource + Send + Sync>,
    ) -> Self {
        Self {
            cache: ToolchainCache::new(cache_root),
            source,
            sdk_iso: SDK_ISO,
            sdk_members: ISO_MEMBERS,
            llvm: llvm_for_host(),
            libtinfo: libtinfo_for_host(),
        }
    }

    /// Point the bootstrap at different artifacts. Test-only: in production the answer to
    /// "which artifacts?" is `docs/pinned-artifacts.md` and nothing else.
    #[cfg(test)]
    fn with_artifacts(
        mut self,
        sdk_iso: PinnedDownload,
        sdk_members: &'static [IsoMember],
        llvm: PinnedLlvm,
        libtinfo: Option<PinnedLibrary>,
    ) -> Self {
        self.sdk_iso = sdk_iso;
        self.sdk_members = sdk_members;
        self.llvm = Some(llvm);
        self.libtinfo = libtinfo;
        self
    }

    pub fn cache(&self) -> &ToolchainCache {
        &self.cache
    }

    /// Return the Toolchain, bootstrapping it first if the cache does not already hold one.
    pub fn ensure(&self, progress: &ProgressReporter) -> Result<Toolchain, ToolchainError> {
        // Before anything else, including the shortcut below: a machine whose toolchain is
        // already built is exactly the machine still holding what an older version of the
        // installer downloaded, and nobody is going to look in there and work out what is
        // safe to delete.
        self.reclaim_superseded_downloads(progress);

        if let Some(toolchain) = self.cache.installed() {
            progress.report(
                Stage::Build,
                format!(
                    "Using the toolchain already set up ({}).",
                    toolchain.identity()
                ),
            );
            return Ok(toolchain);
        }

        // Say how much before starting, and say it from the pinned sizes rather than from a
        // number typed into a sentence - a stale figure here is how a 2.5 GB download comes
        // as a surprise.
        let total = self.sdk_member_bytes()
            + self.llvm.map_or(0, |llvm| llvm.download.approximate_bytes)
            + self
                .libtinfo
                .map_or(0, |library| library.download.approximate_bytes);
        progress.report(
            Stage::Build,
            format!(
                "Setting up the build tools. This happens once and downloads about {:.1} GB.",
                total as f64 / (1024.0 * 1024.0 * 1024.0)
            ),
        );

        fs::create_dir_all(self.cache.root())
            .map_err(|error| io_error("create the toolchain folder", self.cache.root(), &error))?;
        // Whatever an interrupted run left behind is not trusted; the verified downloads
        // survive, so a retry does not pay for the bytes twice.
        self.cache.discard_partial_state()?;

        let downloads = self.cache.downloads_dir();
        let sdk = members::fetch_missing(
            self.source.as_ref(),
            &self.sdk_iso,
            self.sdk_members,
            &downloads.join(SDK_MEMBERS_DIR),
            progress,
        )?;

        let Some(llvm) = self.llvm else {
            return Err(ToolchainError::new(
                "The installer does not have a compiler for this kind of computer, so it \
                 cannot build the mod's DLL here.",
                format!(
                    "no pinned LLVM for {}-{}",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ),
            ));
        };
        let llvm_archive = fetch(self.source.as_ref(), &llvm.download, &downloads, progress)?;

        let sdk_root = self.cache.sdk_root();
        let counts = extract::extract_members(&sdk, self.sdk_members, &sdk_root, progress)?;

        progress.report(Stage::Build, "Unpacking the compiler.");
        tarball::extract_llvm(
            &llvm_archive,
            llvm.archive_root,
            &self.cache.llvm_root(),
            progress,
        )?;

        // The compiler will not start without this, so it goes in before anything tries to run
        // it. Inside the compiler's own `lib/`, which its `RUNPATH: $ORIGIN/../lib` already
        // searches - no environment has to be arranged around every invocation (ADR-0005).
        if let Some(library) = self.libtinfo {
            let package = fetch(
                self.source.as_ref(),
                &library.download,
                &downloads,
                progress,
            )?;
            install_support_library(&package, library, &self.cache.llvm_root())?;
        }

        // Fail here rather than in the compiler: an extraction with no `Include` and no `Lib`
        // anywhere in it produced *something*, so only this check tells the difference
        // between "the wrong members" and "a disk that filled up".
        let roots = sdk_layout::find(&sdk_root)?;
        if roots.is_empty() {
            return Err(ToolchainError::new(
                "The Windows SDK did not unpack completely. Clear the installer's data folder \
                 and try again.",
                format!(
                    "{} contains no Include or Lib folder after unpacking {} files",
                    sdk_root.display(),
                    counts.files_written
                ),
            ));
        }

        fixups::apply(&sdk_root, progress)?;

        let report = verify_extraction(&sdk_root)?;
        require_complete(&report, &sdk_root)?;

        // A finishing line with a figure in it, and nothing else: the header counts, the
        // fix-up tallies and the include and lib paths that used to be here are diagnostics,
        // and the Activity panel is read by players rather than by maintainers.
        progress.report(
            Stage::Build,
            format!(
                "Build tools ready - {} files unpacked ({} MB).",
                counts.files_written,
                counts.bytes_written / (1024 * 1024)
            ),
        );

        self.cache.mark_complete()
    }

    /// How much of the disc image a first bootstrap actually pulls down.
    ///
    /// From the member table this bootstrap was given rather than from the pinned constant,
    /// so the fast suite's sentences describe the fixture it is really fetching.
    fn sdk_member_bytes(&self) -> u64 {
        self.sdk_members
            .iter()
            .flat_map(IsoMember::files)
            .map(|file| file.bytes)
            .sum()
    }

    /// Give back the disk an older version of the installer is still using.
    ///
    /// Up to 0.1.2 the bootstrap downloaded the whole 1.45 GiB disc image and extracted
    /// straight from it, so that is what those installs have sitting in their downloads
    /// folder - plus, on a machine where the download kept failing, a part-finished copy of
    /// it. This version reads eleven files totalling ~102 MiB and never wants the rest, so
    /// the image is turned into those eleven - locally, with nothing downloaded - and then
    /// removed, and the half-finished remains of one go too.
    ///
    /// Deliberately not fatal. This is housekeeping: if the image turns out to be damaged or
    /// the disk refuses the write, the bootstrap carries on and fetches what it needs, which
    /// is what it would have done had the image never been there.
    fn reclaim_superseded_downloads(&self, progress: &ProgressReporter) {
        let downloads = self.cache.downloads_dir();
        let image = downloads.join(self.sdk_iso.file_name);
        let mut freed = 0u64;

        if image.is_file() {
            match members::harvest_from_image(
                &image,
                self.sdk_members,
                &downloads.join(SDK_MEMBERS_DIR),
                progress,
            ) {
                Ok(bytes) => freed += bytes,
                Err(error) => progress.report(
                    Stage::Build,
                    format!(
                        "Could not reuse the Windows SDK already on this computer, so it was \
                         left alone: {}",
                        error.message()
                    ),
                ),
            }
        }

        // A `.part` and its ledger are a download of the whole image that will never be
        // resumed, because nothing asks for the whole image any more. On the machine that
        // reported this, that was 352 MB of nothing.
        for leftover in [
            downloads.join(format!("{}.part", self.sdk_iso.file_name)),
            downloads.join(format!("{}.parts", self.sdk_iso.file_name)),
        ] {
            if let Ok(metadata) = fs::metadata(&leftover)
                && fs::remove_file(&leftover).is_ok()
            {
                freed += metadata.len();
            }
        }

        if freed > 0 {
            progress.report(
                Stage::Build,
                format!(
                    "Freed {:.1} GB of space the build tools no longer need.",
                    freed as f64 / (1024.0 * 1024.0 * 1024.0)
                ),
            );
        }
    }
}

/// Put a support library where the compiler's own `RUNPATH` will find it.
///
/// `$ORIGIN/../lib` is already baked into the llvm.org binaries, so a file dropped in the
/// compiler's `lib/` is found with no `LD_LIBRARY_PATH` and no wrapper script - which matters,
/// because the alternative is arranging an environment around every compiler invocation the
/// toolchain runner ever makes (ADR-0005).
fn install_support_library(
    package: &Path,
    library: PinnedLibrary,
    llvm_root: &Path,
) -> Result<(), ToolchainError> {
    let bytes = deb::extract_from_data_tar(package, library.member)?;

    let lib_dir = llvm_root.join("lib");
    fs::create_dir_all(&lib_dir)
        .map_err(|error| io_error("create the compiler's library folder", &lib_dir, &error))?;

    let installed = lib_dir.join(library.install_as);
    fs::write(&installed, &bytes)
        .map_err(|error| io_error("write a compiler support library", &installed, &error))?;

    // The compiler asks for the SONAME, which every distribution ships as a symlink to the
    // versioned file. A copy would work too, but a link is what the loader expects and costs
    // nothing.
    let link = lib_dir.join(library.link_as);
    let _ = fs::remove_file(&link);
    #[cfg(unix)]
    std::os::unix::fs::symlink(library.install_as, &link)
        .map_err(|error| io_error("link a compiler support library", &link, &error))?;
    #[cfg(not(unix))]
    fs::write(&link, &bytes)
        .map_err(|error| io_error("write a compiler support library", &link, &error))?;

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::download::Transfer;
    use crate::pinned::{ISO_MEMBERS, PinnedMember};
    use crate::test_fixtures;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    use std::io::Cursor;
    use std::sync::Mutex;

    /// Serves the two pinned URLs out of memory, so the whole bootstrap can run in the fast
    /// suite. It also counts requests, which is how "bootstrap runs once" is asserted.
    #[derive(Clone)]
    struct FakeInternet {
        bodies: BTreeMap<String, Vec<u8>>,
        requests: std::sync::Arc<Mutex<usize>>,
    }

    impl FakeInternet {
        fn requests(&self) -> usize {
            self.requests.lock().map(|count| *count).unwrap_or(0)
        }
    }

    impl ByteSource for FakeInternet {
        fn open(&self, url: &str, offset: u64) -> Result<Transfer, ToolchainError> {
            if let Ok(mut count) = self.requests.lock() {
                *count += 1;
            }
            let Some(body) = self.bodies.get(url) else {
                return Err(ToolchainError::new(
                    "Not found.",
                    format!("no body for {url}"),
                ));
            };
            let start = offset.min(body.len() as u64);
            Ok(Transfer {
                start,
                total: Some(body.len() as u64),
                body: Box::new(Cursor::new(body[start as usize..].to_vec())),
            })
        }
    }

    /// A stand-in ISO containing the four pinned members, each a real MSI over real CABs.
    ///
    /// The contents are a hand-picked slice of what the genuine image holds - enough to
    /// exercise the MSI mapping, every fix-up and all six verification names, and small
    /// enough to build in a test. Files are spread one per cabinet so `WinSDKBuild`'s
    /// four-cabinet `Media` table is genuinely routed rather than assumed away.
    fn synthetic_sdk_iso() -> Vec<u8> {
        let mut builder = test_fixtures::iso::IsoBuilder::new();
        for member in ISO_MEMBERS {
            let plan = member_contents(member.msi.path);

            // One `Media` row per pinned cabinet, covering the sequences assigned to it.
            let mut media: Vec<(i32, &str)> = Vec::new();
            let mut last = 0;
            for (index, cab) in member.cabs.iter().enumerate() {
                let name = cab.path.rsplit('/').next().unwrap_or(cab.path);
                last = plan
                    .files
                    .iter()
                    .filter(|(_, cab, _)| *cab == index)
                    .map(|(row, _, _)| row.sequence)
                    .max()
                    .unwrap_or(last);
                media.push((last, name));
            }

            let rows: Vec<test_fixtures::package::FileRow> = plan
                .files
                .iter()
                .map(|(row, _, _)| test_fixtures::package::FileRow {
                    key: row.key,
                    file_name: row.file_name,
                    component: row.component,
                    sequence: row.sequence,
                })
                .collect();
            let msi =
                test_fixtures::package::build(&plan.directories, &plan.components, &rows, &media);
            builder = builder.file(member.msi.path, msi);

            for (index, cab) in member.cabs.iter().enumerate() {
                let payloads: Vec<(&str, &[u8])> = plan
                    .files
                    .iter()
                    .filter(|(_, cab, _)| *cab == index)
                    .map(|(row, _, content)| (row.key, content.as_bytes()))
                    .collect();
                // A cabinet with no rows still has to exist and open: the pre-flight check
                // reads the pinned list, not the `Media` table.
                let payloads = if payloads.is_empty() {
                    vec![("unused", b"unused" as &[u8])]
                } else {
                    payloads
                };
                builder = builder.file(cab.path, test_fixtures::cabinet::build(&payloads));
            }
        }
        builder.build()
    }

    /// One member's tables plus, for each file, which of the member's cabinets holds it and
    /// what it contains.
    struct MemberPlan {
        directories: Vec<test_fixtures::package::DirectoryRow>,
        components: Vec<(&'static str, &'static str)>,
        files: Vec<(test_fixtures::package::FileRow, usize, &'static str)>,
    }

    fn member_contents(msi_path: &str) -> MemberPlan {
        use test_fixtures::package::{DirectoryRow, FileRow};

        let root = vec![
            DirectoryRow {
                key: "TARGETDIR",
                parent: "",
                default_dir: "SourceDir",
            },
            DirectoryRow {
                key: "SDKROOT",
                parent: "TARGETDIR",
                default_dir: ".",
            },
        ];
        let mut directories = root;

        match msi_path {
            // Headers and import libraries: the bulk of the SDK.
            "Setup/WinSDKBuild/WinSDKBuild_x86.msi" => {
                directories.extend([
                    DirectoryRow {
                        key: "IncludeDir",
                        parent: "SDKROOT",
                        default_dir: "Include",
                    },
                    DirectoryRow {
                        key: "SubDir",
                        parent: "IncludeDir",
                        default_dir: "Sub",
                    },
                    DirectoryRow {
                        key: "LibDir",
                        parent: "SDKROOT",
                        default_dir: "Lib",
                    },
                ]);
                MemberPlan {
                    directories,
                    components: vec![
                        ("IncCmp", "IncludeDir"),
                        ("SubCmp", "SubDir"),
                        ("LibCmp", "LibDir"),
                    ],
                    files: vec![
                        (
                            FileRow {
                                key: "f1",
                                file_name: "windows.h",
                                component: "IncCmp",
                                sequence: 1,
                            },
                            0,
                            // Reaches for a mixed-case name, a WDK-only one, and a
                            // backslashed path: fix-ups 1, 2, 4 and 6 all have work to do.
                            "#include <WinDef.h>\n#include <DriverSpecs.h>\n#include <sub\\Extra.h>\n",
                        ),
                        (
                            FileRow {
                                key: "f2",
                                file_name: "WINDEF~1.H|WinDef.h",
                                component: "IncCmp",
                                sequence: 2,
                            },
                            1,
                            "/* windef */\n",
                        ),
                        (
                            FileRow {
                                key: "f3",
                                file_name: "Kernel32.Lib",
                                component: "LibCmp",
                                sequence: 3,
                            },
                            2,
                            "kernel32 import library\n",
                        ),
                        (
                            FileRow {
                                key: "f4",
                                file_name: "Extra.h",
                                component: "SubCmp",
                                sequence: 4,
                            },
                            3,
                            "/* extra */\n",
                        ),
                    ],
                }
            }
            // The VC9 CRT, which is where `stdio.h`, `iostream` and `msvcrt.lib` come from.
            "Setup/vc_stdx86/vc_stdx86.msi" => {
                directories.extend([
                    DirectoryRow {
                        key: "VcDir",
                        parent: "SDKROOT",
                        default_dir: "VC",
                    },
                    DirectoryRow {
                        key: "VcInclude",
                        parent: "VcDir",
                        default_dir: "include",
                    },
                    DirectoryRow {
                        key: "VcLib",
                        parent: "VcDir",
                        default_dir: "lib",
                    },
                ]);
                MemberPlan {
                    directories,
                    components: vec![("CrtInc", "VcInclude"), ("CrtLib", "VcLib")],
                    files: vec![
                        (
                            FileRow {
                                key: "c1",
                                file_name: "stdio.h",
                                component: "CrtInc",
                                sequence: 1,
                            },
                            0,
                            "/* stdio */\n",
                        ),
                        (
                            FileRow {
                                key: "c2",
                                file_name: "iostream",
                                component: "CrtInc",
                                sequence: 2,
                            },
                            0,
                            "/* iostream */\n",
                        ),
                        (
                            FileRow {
                                key: "c3",
                                file_name: "msvcrt.lib",
                                component: "CrtLib",
                                sequence: 3,
                            },
                            0,
                            "msvcrt import library\n",
                        ),
                    ],
                }
            }
            // The two members that contribute little the verification looks at, but which
            // must still parse and extract or the bootstrap stops.
            _ => {
                directories.push(DirectoryRow {
                    key: "BinDir",
                    parent: "SDKROOT",
                    default_dir: "Bin",
                });
                MemberPlan {
                    directories,
                    components: vec![("BinCmp", "BinDir")],
                    files: vec![(
                        FileRow {
                            key: "b1",
                            file_name: "rc.exe",
                            component: "BinCmp",
                            sequence: 1,
                        },
                        0,
                        "not really an executable\n",
                    )],
                }
            }
        }
    }

    /// A stand-in LLVM tarball with the handful of members the extraction filter keeps.
    fn synthetic_llvm_tarball(root: &str) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, content) in [
            ("bin/clang-cl", "clang driver"),
            ("bin/lld-link", "lld"),
            ("lib/clang/18/include/stddef.h", "builtin header"),
            ("lib/libclang-cpp.so.18.1", "shared"),
            ("lib/libLLVMCore.a", "dropped"),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            let _ = builder.append_data(&mut header, format!("{root}/{name}"), content.as_bytes());
        }
        let tar_bytes = builder.into_inner().unwrap_or_default();
        let mut compressed = Vec::new();
        let _ = lzma_rs::xz_compress(&mut tar_bytes.as_slice(), &mut compressed);
        compressed
    }

    /// Wire the fixtures up as the two pinned URLs, with the checksums they actually hash to.
    ///
    /// The pinned constants hold `&'static str` digests, so the fixtures' own digests are
    /// leaked to stand in for them. Everything else - including which URLs are asked for - is
    /// what production does.
    /// Measure the fixture image the way `describe_the_pinned_members` measures the real one.
    ///
    /// The offsets and checksums in `pinned.rs` came off a real image; a fixture needs its
    /// own, because it is a different image with the same member paths. Measuring rather than
    /// hard-coding is also what keeps the fast suite honest about the windowed download: the
    /// bootstrap under test asks for byte ranges nobody typed in by hand.
    fn measure_members(image: &[u8]) -> &'static [IsoMember] {
        let mut disc = crate::disc::Disc::open(Cursor::new(image.to_vec())).unwrap();
        let mut measured = Vec::new();
        for member in ISO_MEMBERS {
            let mut measure = |path: &'static str| {
                let extents = disc.extents(path).unwrap();
                let bytes = disc.read_file(path).unwrap();
                assert_eq!(
                    extents.len(),
                    1,
                    "{path} is in {} pieces; a window cannot fetch it",
                    extents.len()
                );
                PinnedMember {
                    path,
                    offset: extents[0].0,
                    bytes: bytes.len() as u64,
                    sha256: leak_digest(&bytes),
                }
            };
            let msi = measure(member.msi.path);
            let cabs: Vec<PinnedMember> = member.cabs.iter().map(|cab| measure(cab.path)).collect();
            measured.push(IsoMember {
                msi,
                cabs: Box::leak(cabs.into_boxed_slice()),
                label: member.label,
            });
        }
        Box::leak(measured.into_boxed_slice())
    }

    fn fake_internet() -> (
        FakeInternet,
        PinnedDownload,
        &'static [IsoMember],
        PinnedLlvm,
        Option<PinnedLibrary>,
    ) {
        let iso = synthetic_sdk_iso();
        let mut sdk = SDK_ISO;
        sdk.sha256 = leak_digest(&iso);
        let members = measure_members(&iso);

        let mut llvm = llvm_for_host().unwrap_or(PinnedLlvm {
            download: PinnedDownload {
                file_name: "llvm.tar.xz",
                url: "https://example.invalid/llvm.tar.xz",
                sha256: "",
                approximate_bytes: 0,
            },
            archive_root: "llvm-root",
        });
        let tarball = synthetic_llvm_tarball(llvm.archive_root);
        llvm.download.sha256 = leak_digest(&tarball);

        let mut bodies = BTreeMap::from([
            (sdk.url.to_string(), iso),
            (llvm.download.url.to_string(), tarball),
        ]);

        // The support library is fetched on hosts that need one, so the fixture serves it on
        // exactly those hosts - a `.deb` built the same way the real one is packaged.
        let libtinfo = libtinfo_for_host().map(|mut library| {
            let package = test_fixtures::deb::package(library.member, b"not really ncurses");
            library.download.sha256 = leak_digest(&package);
            bodies.insert(library.download.url.to_string(), package);
            library
        });

        (
            FakeInternet {
                bodies,
                requests: std::sync::Arc::new(Mutex::new(0)),
            },
            sdk,
            members,
            llvm,
            libtinfo,
        )
    }

    fn leak_digest(bytes: &[u8]) -> &'static str {
        let digest = Sha256::digest(bytes);
        let hex = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Box::leak(hex.into_boxed_str())
    }

    /// The production [`ToolchainBootstrap`], pointed at the fixtures.
    fn bootstrap_over(
        root: &std::path::Path,
        internet: &FakeInternet,
        sdk: PinnedDownload,
        members: &'static [IsoMember],
        llvm: PinnedLlvm,
        libtinfo: Option<PinnedLibrary>,
    ) -> ToolchainBootstrap {
        ToolchainBootstrap::with_byte_source(root.to_path_buf(), Box::new(internet.clone()))
            .with_artifacts(sdk, members, llvm, libtinfo)
    }

    /// The whole sequence, end to end, with no network: download, ISO, MSI, CAB, fix-ups,
    /// verification, marker.
    #[test]
    fn a_bootstrap_produces_a_verified_toolchain() {
        let dir = tempfile::tempdir().unwrap();
        let (internet, sdk, members, llvm, libtinfo) = fake_internet();

        let toolchain = bootstrap_over(dir.path(), &internet, sdk, members, llvm, libtinfo)
            .ensure(&ProgressReporter::silent())
            .unwrap();

        assert!(toolchain.identity().starts_with("clang-18.1.8"));
        // Every name from `docs/pinned-artifacts.md` §4 resolves.
        let report = verify_extraction(toolchain.sdk_root()).unwrap();
        assert!(report.is_complete(), "{}", report.summary());
        // The compiler is unpacked and filtered.
        assert!(toolchain.llvm_root().join("bin/clang-cl").exists());
        assert!(!toolchain.llvm_root().join("lib/libLLVMCore.a").exists());
        assert!(!dir.path().join("staging").exists());
    }

    /// The MSI's mapping is honoured: names inside the CAB are `f1`/`c2`, names on disk are
    /// the ones the Directory and File tables spell out.
    #[test]
    fn files_land_where_the_msi_says_not_where_the_cab_names_them() {
        let dir = tempfile::tempdir().unwrap();
        let (internet, sdk, members, llvm, libtinfo) = fake_internet();

        let toolchain = bootstrap_over(dir.path(), &internet, sdk, members, llvm, libtinfo)
            .ensure(&ProgressReporter::silent())
            .unwrap();

        let sdk_root = toolchain.sdk_root();
        assert!(sdk_root.join("Include/windows.h").exists());
        assert!(sdk_root.join("VC/include/iostream").exists());
        assert!(sdk_root.join("Bin/rc.exe").exists());
        // Nothing is ever written under a CAB-internal name.
        assert!(!sdk_root.join("Include/f1").exists());
        assert!(!sdk_root.join("f1").exists());
    }

    /// The point of the member table: the 1.45 GiB image is never downloaded at all.
    #[test]
    fn a_bootstrap_fetches_the_pinned_members_and_not_the_image() {
        let dir = tempfile::tempdir().unwrap();
        let (internet, sdk, members, llvm, libtinfo) = fake_internet();

        bootstrap_over(dir.path(), &internet, sdk, members, llvm, libtinfo)
            .ensure(&ProgressReporter::silent())
            .unwrap();

        let downloads = ToolchainCache::new(dir.path().to_path_buf()).downloads_dir();
        assert!(
            !downloads.join(sdk.file_name).exists(),
            "the whole disc image must never be downloaded"
        );
        let staged: Vec<String> = std::fs::read_dir(downloads.join(SDK_MEMBERS_DIR))
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            staged.len(),
            members.iter().flat_map(IsoMember::files).count(),
            "every pinned member should be here and nothing else: {staged:?}"
        );
        // Flattened, so the two different `cab1.cab`s do not land on each other.
        assert!(staged.contains(&"WinSDK-cab1.cab".to_owned()), "{staged:?}");
        assert!(
            staged.contains(&"WinSDKBuild-cab1.cab".to_owned()),
            "{staged:?}"
        );
    }

    /// An image downloaded by an earlier version of the installer is turned into the eleven
    /// packages this one needs - locally, with nothing downloaded - and then removed.
    ///
    /// This is the upgrade path: 0.1.2 left a 1.45 GiB file in the downloads folder that
    /// nothing reads any more, and a player is not going to work out for themselves which of
    /// those files are safe to delete.
    #[test]
    fn an_image_from_an_older_installer_becomes_the_packages_and_is_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let (internet, sdk, members, llvm, libtinfo) = fake_internet();
        let cache = ToolchainCache::new(dir.path().to_path_buf());
        let image = cache.downloads_dir().join(sdk.file_name);
        std::fs::create_dir_all(cache.downloads_dir()).unwrap();
        std::fs::write(&image, internet.bodies.get(sdk.url).unwrap()).unwrap();

        bootstrap_over(dir.path(), &internet, sdk, members, llvm, libtinfo)
            .ensure(&ProgressReporter::silent())
            .unwrap();

        assert!(!image.exists(), "the superseded image must be removed");
        let staged: Vec<String> = std::fs::read_dir(cache.downloads_dir().join(SDK_MEMBERS_DIR))
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            staged.len(),
            members.iter().flat_map(IsoMember::files).count(),
            "every package should have come out of the image: {staged:?}"
        );
        // Only the compiler and, where the host needs it, the support library travelled: the
        // packages came off the disk, not the network.
        let expected = 2 + usize::from(libtinfo_for_host().is_some());
        assert_eq!(internet.requests(), expected);
    }

    /// The same tidy-up on a machine that already has a working toolchain - which is most of
    /// the machines holding a superseded image, since they only kept it by finishing.
    #[test]
    fn a_superseded_image_is_reclaimed_even_when_nothing_needs_building() {
        let dir = tempfile::tempdir().unwrap();
        let (internet, sdk, members, llvm, libtinfo) = fake_internet();
        bootstrap_over(dir.path(), &internet, sdk, members, llvm, libtinfo)
            .ensure(&ProgressReporter::silent())
            .unwrap();

        // As if this cache had been built by the older installer: the image is back, and so
        // is the half-finished download of it that a flaky connection left behind.
        let cache = ToolchainCache::new(dir.path().to_path_buf());
        let image = cache.downloads_dir().join(sdk.file_name);
        std::fs::write(&image, internet.bodies.get(sdk.url).unwrap()).unwrap();
        let partial = cache
            .downloads_dir()
            .join(format!("{}.part", sdk.file_name));
        let ledger = cache
            .downloads_dir()
            .join(format!("{}.parts", sdk.file_name));
        std::fs::write(&partial, vec![0u8; 4096]).unwrap();
        std::fs::write(&ledger, "chunks v1 65536 300000\n").unwrap();

        let (sender, receiver) = std::sync::mpsc::channel();
        bootstrap_over(dir.path(), &internet, sdk, members, llvm, libtinfo)
            .ensure(&ProgressReporter::to_channel(sender))
            .unwrap();

        assert!(!image.exists(), "the image is not needed by this version");
        assert!(!partial.exists(), "a download nobody will ever resume");
        assert!(!ledger.exists());
        let messages: Vec<String> = receiver.iter().map(|event| event.message).collect();
        assert!(
            messages.iter().any(|message| message.contains("Freed")),
            "the reclaimed space should be reported: {messages:?}"
        );
    }

    /// A damaged image is not something to build from, and not something to delete on the
    /// strength of: the run says so and fetches the packages instead.
    #[test]
    fn a_damaged_image_from_an_older_installer_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let (internet, sdk, members, llvm, libtinfo) = fake_internet();
        let cache = ToolchainCache::new(dir.path().to_path_buf());
        let image = cache.downloads_dir().join(sdk.file_name);
        std::fs::create_dir_all(cache.downloads_dir()).unwrap();
        let mut damaged = internet.bodies.get(sdk.url).unwrap().clone();
        damaged.truncate(20 * 2048);
        std::fs::write(&image, &damaged).unwrap();

        bootstrap_over(dir.path(), &internet, sdk, members, llvm, libtinfo)
            .ensure(&ProgressReporter::silent())
            .unwrap();

        assert!(
            image.exists(),
            "a file we could not read is not ours to delete"
        );
        // Everything the build needed was fetched instead.
        let members_fetched: usize = members.iter().flat_map(IsoMember::files).count();
        let expected = members_fetched + 2 + usize::from(libtinfo_for_host().is_some());
        assert_eq!(internet.requests(), expected);
    }

    /// Bootstrap runs once; subsequent builds detect the populated Toolchain Cache and
    /// skip it.
    #[test]
    fn a_second_bootstrap_does_nothing_at_all() {
        let dir = tempfile::tempdir().unwrap();
        let (internet, sdk, members, llvm, libtinfo) = fake_internet();
        let first = bootstrap_over(dir.path(), &internet, sdk, members, llvm, libtinfo)
            .ensure(&ProgressReporter::silent())
            .unwrap();
        // One request per pinned member of the disc image - the whole image is never asked
        // for - plus the compiler, which is pinned large and so costs a parallel-path probe
        // that discovers the fixture is tiny and drops to the sequential path. The libtinfo
        // package, where the host needs one, is pinned small and fetches in one.
        let members_fetched: usize = members.iter().flat_map(IsoMember::files).count();
        let expected = members_fetched + 2 + usize::from(libtinfo_for_host().is_some());
        assert_eq!(internet.requests(), expected);

        let (second_internet, sdk, members, llvm, libtinfo) = fake_internet();
        let second = bootstrap_over(dir.path(), &second_internet, sdk, members, llvm, libtinfo)
            .ensure(&ProgressReporter::silent())
            .unwrap();

        assert_eq!(second, first);
        assert_eq!(
            second_internet.requests(),
            0,
            "the cache must be used, not refetched"
        );
    }

    /// An interrupted bootstrap leaves a state that self-repairs on retry.
    #[test]
    fn a_bootstrap_interrupted_halfway_repairs_itself() {
        let dir = tempfile::tempdir().unwrap();
        let (internet, sdk, members, llvm, libtinfo) = fake_internet();
        bootstrap_over(dir.path(), &internet, sdk, members, llvm, libtinfo)
            .ensure(&ProgressReporter::silent())
            .unwrap();

        // Simulate being killed after extraction started but before the marker: the marker
        // goes, half the headers go, and a stale CAB is left in staging.
        let cache = ToolchainCache::new(dir.path().to_path_buf());
        fs::remove_file(dir.path().join(".toolchain-complete")).unwrap();
        fs::remove_file(cache.sdk_root().join("Include/windows.h")).unwrap();
        fs::create_dir_all(cache.staging_dir()).unwrap();
        fs::write(cache.staging_dir().join("staged-cab1.cab"), b"stale").unwrap();
        assert!(cache.installed().is_none());

        let (retry_internet, sdk, members, llvm, libtinfo) = fake_internet();
        let repaired = bootstrap_over(dir.path(), &retry_internet, sdk, members, llvm, libtinfo)
            .ensure(&ProgressReporter::silent())
            .unwrap();

        assert!(cache.sdk_root().join("Include/windows.h").exists());
        assert!(
            verify_extraction(repaired.sdk_root())
                .unwrap()
                .is_complete()
        );
        assert!(!cache.staging_dir().exists());
        // The verified downloads survived, so nothing was fetched again.
        assert_eq!(retry_internet.requests(), 0);
    }

    /// A bootstrap over a damaged ISO must fail loudly and leave no marker - the next attempt
    /// has to be able to tell that this one did not finish.
    #[test]
    fn a_damaged_iso_leaves_no_toolchain_behind() {
        let dir = tempfile::tempdir().unwrap();
        let (mut internet, sdk, members, llvm, libtinfo) = fake_internet();
        if let Some(body) = internet.bodies.get_mut(sdk.url) {
            // Truncate the ISO's payload area, leaving the descriptors intact.
            body.truncate(20 * 2048);
        }

        let error = bootstrap_over(dir.path(), &internet, sdk, members, llvm, libtinfo)
            .ensure(&ProgressReporter::silent())
            .unwrap_err();

        assert!(!error.message().is_empty());
        assert!(
            ToolchainCache::new(dir.path().to_path_buf())
                .installed()
                .is_none()
        );
    }

    /// No user-facing sentence may be raw jargon.
    #[test]
    fn progress_reaches_the_user_in_plain_language() {
        let dir = tempfile::tempdir().unwrap();
        let (internet, sdk, members, llvm, libtinfo) = fake_internet();
        let (sender, receiver) = std::sync::mpsc::channel();

        bootstrap_over(dir.path(), &internet, sdk, members, llvm, libtinfo)
            .ensure(&ProgressReporter::to_channel(sender))
            .unwrap();

        let messages: Vec<String> = receiver.iter().map(|event| event.message).collect();
        assert!(!messages.is_empty());
        assert!(
            messages
                .iter()
                .any(|message| message.contains("Downloading")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|message| message.contains("Unpacking")),
            "{messages:?}"
        );
        for message in &messages {
            assert!(
                !message.contains("ISO9660")
                    && !message.contains("msi")
                    && !message.contains("sha256"),
                "not plain language: {message}"
            );
        }
    }
}
