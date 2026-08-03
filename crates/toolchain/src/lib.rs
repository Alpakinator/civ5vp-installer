//! Toolchain Bootstrap for the Civ 5 VP Installer.
//!
//! First time a build is needed, this crate makes the Toolchain exist: it downloads the two
//! pinned artifacts with visible progress, extracts the Windows SDK 7.0 and the VC9 CRT out
//! of the disc image **in process** (UDF or ISO9660 → MSI → CAB; no 7-Zip, no msitools, no
//! wine — rule 5 and ADR-0001), applies the Linux case-folding fix-ups, and leaves the result
//! in the Toolchain Cache. Every later build finds the cache populated and skips all of it.
//!
//! ADR-0001 and `docs/pinned-artifacts.md` both say the image is ISO9660. It is not: the
//! pinned `GRMSDK_EN_DVD.iso` is a UDF disc whose ISO9660 side holds one `README.TXT` saying
//! so. Both readers exist; [`disc`] picks by probing. See [`udf`] for the detail.
//!
//! It is a separate crate from the Core for one reason: the Core has no dependencies and must
//! keep having none (rule 1), while this needs an HTTP client and three archive parsers.
//! Nothing here depends on egui either — this crate is as headless as the Core is.
//!
//! Everything it is allowed to fetch is in [`pinned`], transcribed from
//! `docs/pinned-artifacts.md`.

// Rule 9: this is reachable from the UI, through the `ToolchainRunner` boundary.
//
// Unlike the Core, this crate's tests live *inside* it — the parsers are internal and there
// is no public API to reach them through — so each `mod tests` carries the `allow` rule 9
// grants tests explicitly. Nothing outside a `#[cfg(test)]` module may.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented
)]

mod bootstrap;
mod cabinet;
mod cache;
mod disc;
mod download;
mod error;
mod extract;
mod fixups;
mod iso9660;
mod msi_layout;
pub mod pinned;
mod runner;
mod sdk_layout;
mod tarball;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod test_fixtures;
mod udf;
mod verify;

pub use bootstrap::ToolchainBootstrap;
pub use cache::{Toolchain, ToolchainCache};
pub use error::ToolchainError;
pub use runner::BootstrappedToolchain;
pub use verify::{Baseline, ExtractionReport, REFERENCE_BASELINE, verify_extraction};
