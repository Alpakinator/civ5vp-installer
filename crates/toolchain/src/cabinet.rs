//! Reading a CAB, one sequential pass per folder.
//!
//! The `cab` crate parses and decompresses this format correctly, and this module started as
//! a wrapper over it. It is hand-rolled instead for one reason: `cab::Cabinet::read_file`
//! builds a fresh folder reader per call and decompresses the folder from its start every
//! time, so extracting *N* files out of one folder costs O(N × folder size). That is fine for
//! the two-file cabinets its API was shaped for and quadratic here — `WinSDKBuild_x86.msi`
//! routes 2836 files through four cabinets of one ~52 MB LZX folder each, which comes to
//! roughly 69 GB of decompression to extract 168 MB.
//!
//! Reading a folder once, in order, and writing each file as its bytes go past costs the
//! 168 MB. The format makes that easy: a folder's data is a chain of blocks, and its files
//! sit end to end in the decompressed stream at offsets the file table gives.
//!
//! Decompression is *not* hand-rolled — `flate2` does MSZIP's deflate and `lzxd` does LZX,
//! which is what the `cab` crate delegates to as well. Quantum is refused: nothing implements
//! it in Rust and the Microsoft cabinets here do not use it.
//!
//! The reader is cross-checked against `cab` rather than trusted: the fast suite round-trips
//! MSZIP cabinets written by `cab`'s own builder and compares both readers member for member,
//! and the `#[ignore]`d disc-image diagnostic does the same on the real LZX cabinets.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::error::{ToolchainError, io_error, missing_member};

/// `CFHEADER.signature`.
const SIGNATURE: [u8; 4] = *b"MSCF";
/// Fixed part of `CFHEADER`, before any of the optional fields.
const HEADER_LENGTH: usize = 36;

/// `CFHEADER.flags`.
const FLAG_PREV_CABINET: u16 = 0x0001;
const FLAG_NEXT_CABINET: u16 = 0x0002;
const FLAG_RESERVE_PRESENT: u16 = 0x0004;

/// `CFFILE.attribs`: the name is UTF-8 rather than the local code page.
const ATTRIBUTE_NAME_IS_UTF: u16 = 0x80;

/// `CFDATA` header, before the per-cabinet reserve area.
const DATA_HEADER_LENGTH: usize = 8;

/// How a folder's blocks are compressed.
enum Compression {
    None,
    MsZip(MsZip),
    Lzx(Box<lzxd::Lzxd>),
}

impl Compression {
    /// Decode `CFFOLDER.typeCompress`.
    fn parse(bits: u16) -> Result<Self, ToolchainError> {
        match bits & 0x000F {
            0 => Ok(Self::None),
            1 => Ok(Self::MsZip(MsZip::new())),
            2 => Err(ToolchainError::new(
                "This Windows SDK download uses a compression the installer cannot read. \
                 Please report this — the installer cannot continue.",
                format!("cabinet folder uses Quantum compression (typeCompress {bits:#06x})"),
            )),
            3 => {
                let window = match (bits & 0x1F00) >> 8 {
                    15 => lzxd::WindowSize::KB32,
                    16 => lzxd::WindowSize::KB64,
                    17 => lzxd::WindowSize::KB128,
                    18 => lzxd::WindowSize::KB256,
                    19 => lzxd::WindowSize::KB512,
                    20 => lzxd::WindowSize::MB1,
                    21 => lzxd::WindowSize::MB2,
                    22 => lzxd::WindowSize::MB4,
                    23 => lzxd::WindowSize::MB8,
                    24 => lzxd::WindowSize::MB16,
                    25 => lzxd::WindowSize::MB32,
                    other => {
                        return Err(unreadable(&format!("invalid LZX window size {other}")));
                    }
                };
                Ok(Self::Lzx(Box::new(lzxd::Lzxd::new(window))))
            }
            other => Err(unreadable(&format!("invalid compression type {other}"))),
        }
    }

    fn decompress(&mut self, block: &[u8], uncompressed: usize) -> Result<Vec<u8>, ToolchainError> {
        match self {
            Self::None => Ok(block.to_vec()),
            Self::MsZip(decoder) => decoder.decompress(block, uncompressed),
            Self::Lzx(decoder) => decoder
                .decompress_next(block, uncompressed)
                .map(<[u8]>::to_vec)
                .map_err(|error| unreadable(&format!("LZX decompression failed: {error:?}"))),
        }
    }
}

/// Deflate's window, and so the most history one MSZIP block can refer back to.
const DEFLATE_WINDOW: usize = 0x8000;

/// MSZIP: each block is a `CK` signature followed by a raw deflate stream, and each block's
/// dictionary is the previous block's last 32 KB.
///
/// `flate2` offers no way to hand a decompressor a starting dictionary, so the window is
/// primed the way the format itself would have filled it — by feeding a stored (literal)
/// deflate block holding the previous output, whose bytes are then discarded.
struct MsZip {
    decompressor: flate2::Decompress,
    dictionary: Vec<u8>,
}

impl MsZip {
    fn new() -> Self {
        Self {
            decompressor: flate2::Decompress::new(false),
            dictionary: Vec::with_capacity(DEFLATE_WINDOW),
        }
    }

    fn decompress(&mut self, block: &[u8], uncompressed: usize) -> Result<Vec<u8>, ToolchainError> {
        let Some(payload) = block.strip_prefix(b"CK") else {
            return Err(unreadable(
                "an MSZIP block does not start with its CK signature",
            ));
        };

        self.decompressor.reset(false);
        if !self.dictionary.is_empty() {
            // A stored deflate block: one header byte, then the length and its complement.
            let length = self.dictionary.len() as u16;
            let mut primer = Vec::with_capacity(5 + self.dictionary.len());
            primer.push(0);
            primer.extend_from_slice(&length.to_le_bytes());
            primer.extend_from_slice(&(!length).to_le_bytes());
            primer.extend_from_slice(&self.dictionary);
            let mut discarded = Vec::with_capacity(self.dictionary.len());
            self.decompressor
                .decompress_vec(&primer, &mut discarded, flate2::FlushDecompress::Sync)
                .map_err(|error| {
                    unreadable(&format!("priming the MSZIP window failed: {error}"))
                })?;
        }

        let mut out = Vec::with_capacity(uncompressed);
        self.decompressor
            .decompress_vec(payload, &mut out, flate2::FlushDecompress::Finish)
            .map_err(|error| unreadable(&format!("MSZIP decompression failed: {error}")))?;
        if out.len() != uncompressed {
            return Err(unreadable(&format!(
                "an MSZIP block decompressed to {} bytes, not the {uncompressed} it declares",
                out.len()
            )));
        }

        // Carry the last 32 KB forward as the next block's dictionary.
        if out.len() >= DEFLATE_WINDOW {
            self.dictionary = out[out.len() - DEFLATE_WINDOW..].to_vec();
        } else {
            let total = self.dictionary.len() + out.len();
            if total > DEFLATE_WINDOW {
                self.dictionary.drain(..total - DEFLATE_WINDOW);
            }
            self.dictionary.extend_from_slice(&out);
        }
        Ok(out)
    }
}

/// One `CFFOLDER`.
struct Folder {
    /// Absolute offset of the folder's first `CFDATA`.
    first_block: u64,
    blocks: u16,
    type_compress: u16,
}

/// One `CFFILE`.
struct Member {
    name: String,
    size: u64,
    /// Where this file starts in its folder's decompressed stream.
    offset: u64,
    folder: usize,
}

/// An open cabinet on disk.
pub struct Cabinet {
    reader: BufReader<File>,
    label: String,
    /// `CFHEADER.cbCFData`: a per-block reserve area the format lets a producer insert.
    data_reserve: usize,
    folders: Vec<Folder>,
    members: Vec<Member>,
}

/// One file to pull out, and where to put it.
pub struct Wanted<'a> {
    /// The name inside the cabinet — for an MSI's payload, the `File` key.
    pub name: &'a str,
    pub destination: PathBuf,
}

/// What one extraction pass wrote.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Extracted {
    pub files: usize,
    pub bytes: u64,
}

impl Cabinet {
    pub fn open(path: &Path) -> Result<Self, ToolchainError> {
        let file = File::open(path).map_err(|error| io_error("open a cabinet", path, &error))?;
        let mut reader = BufReader::new(file);
        let label = path.display().to_string();

        let mut header = [0u8; HEADER_LENGTH];
        reader
            .read_exact(&mut header)
            .map_err(|error| io_error("read a cabinet header", path, &error))?;
        if header[0..4] != SIGNATURE {
            return Err(ToolchainError::new(
                "The Windows SDK download could not be unpacked. Clear the installer's data \
                 folder and try again.",
                format!("{label} is not a readable cabinet: no MSCF signature"),
            ));
        }

        let first_file_offset = u64::from(le_u32(&header, 16)?);
        let folder_count = le_u16(&header, 26)? as usize;
        let file_count = le_u16(&header, 28)? as usize;
        let flags = le_u16(&header, 30)?;

        // The optional fields, in the order the format puts them.
        let (mut folder_reserve, mut data_reserve) = (0usize, 0usize);
        if flags & FLAG_RESERVE_PRESENT != 0 {
            let mut sizes = [0u8; 4];
            reader
                .read_exact(&mut sizes)
                .map_err(|error| io_error("read a cabinet header", path, &error))?;
            let header_reserve = i64::from(u16::from_le_bytes([sizes[0], sizes[1]]));
            folder_reserve = sizes[2] as usize;
            data_reserve = sizes[3] as usize;
            reader
                .seek(SeekFrom::Current(header_reserve))
                .map_err(|error| io_error("read a cabinet header", path, &error))?;
        }
        for flag in [FLAG_PREV_CABINET, FLAG_NEXT_CABINET] {
            if flags & flag != 0 {
                read_string(&mut reader, path, false)?;
                read_string(&mut reader, path, false)?;
            }
        }

        let mut folders = Vec::with_capacity(folder_count);
        for _ in 0..folder_count {
            let mut entry = [0u8; 8];
            reader
                .read_exact(&mut entry)
                .map_err(|error| io_error("read a cabinet folder table", path, &error))?;
            folders.push(Folder {
                first_block: u64::from(le_u32(&entry, 0)?),
                blocks: le_u16(&entry, 4)?,
                type_compress: le_u16(&entry, 6)?,
            });
            if folder_reserve > 0 {
                reader
                    .seek(SeekFrom::Current(folder_reserve as i64))
                    .map_err(|error| io_error("read a cabinet folder table", path, &error))?;
            }
        }

        reader
            .seek(SeekFrom::Start(first_file_offset))
            .map_err(|error| io_error("read a cabinet file table", path, &error))?;
        let mut members = Vec::with_capacity(file_count);
        for _ in 0..file_count {
            let mut entry = [0u8; 16];
            reader
                .read_exact(&mut entry)
                .map_err(|error| io_error("read a cabinet file table", path, &error))?;
            let size = u64::from(le_u32(&entry, 0)?);
            let offset = u64::from(le_u32(&entry, 4)?);
            let folder = le_u16(&entry, 8)? as usize;
            let attributes = le_u16(&entry, 14)?;
            let name = read_string(&mut reader, path, attributes & ATTRIBUTE_NAME_IS_UTF != 0)?;

            // Folder indices 0xFFFD and 0xFFFE mark a file continuing from or into another
            // cabinet of the set. The Microsoft cabinets here never do it, and guessing at a
            // neighbour's contents would be worse than saying so.
            if folder >= folders.len() {
                return Err(ToolchainError::new(
                    "The Windows SDK download could not be unpacked. Clear the installer's \
                     data folder and try again.",
                    format!("{name} in {label} continues into another cabinet ({folder:#06x})"),
                ));
            }
            members.push(Member {
                name,
                size,
                offset,
                folder,
            });
        }

        Ok(Self {
            reader,
            label,
            data_reserve,
            folders,
            members,
        })
    }

    /// Extract the named members, reading each folder exactly once.
    ///
    /// Files come out in folder order regardless of the order `wanted` gives them, because
    /// that is the order the decompressor produces them in.
    ///
    /// Names are matched exactly: the MSI's `File` key *is* the CAB-internal name, so an
    /// inexact match would mean the layout mapping was wrong and would put the wrong bytes on
    /// disk. A name this cabinet does not hold is an error, never a silent skip.
    pub fn extract(&mut self, wanted: &[Wanted<'_>]) -> Result<Extracted, ToolchainError> {
        // Resolve every name first, so a name that is not there fails before anything is
        // written.
        let mut by_folder: BTreeMap<usize, Vec<(usize, &Wanted<'_>)>> = BTreeMap::new();
        for want in wanted {
            let index = self
                .members
                .iter()
                .position(|member| member.name == want.name)
                .ok_or_else(|| {
                    missing_member(
                        want.name,
                        &format!("{} (needed for {})", self.label, want.destination.display()),
                    )
                })?;
            by_folder
                .entry(self.members[index].folder)
                .or_default()
                .push((index, want));
        }

        let mut extracted = Extracted::default();
        for (folder, mut files) in by_folder {
            files.sort_by_key(|(index, _)| self.members[*index].offset);
            extracted = self.extract_folder(folder, &files, extracted)?;
        }
        Ok(extracted)
    }

    /// Decompress one folder from its start, writing each wanted file as its bytes go past.
    fn extract_folder(
        &mut self,
        folder_index: usize,
        files: &[(usize, &Wanted<'_>)],
        mut extracted: Extracted,
    ) -> Result<Extracted, ToolchainError> {
        let Some(folder) = self.folders.get(folder_index) else {
            return Err(unreadable(&format!("no folder {folder_index}")));
        };
        let (first_block, blocks, type_compress) =
            (folder.first_block, folder.blocks, folder.type_compress);
        let mut compression = Compression::parse(type_compress)?;

        // The last byte any wanted file needs. Everything past it is not worth decompressing.
        let last_needed = files
            .iter()
            .map(|(index, _)| self.members[*index].offset + self.members[*index].size)
            .max()
            .unwrap_or(0);

        self.reader
            .seek(SeekFrom::Start(first_block))
            .map_err(|error| stream(&self.label, "seek to a cabinet folder", &error))?;

        let mut position = 0u64;
        let mut cursor = 0usize;
        let mut open: Option<BufWriter<File>> = None;

        for _ in 0..blocks {
            if position >= last_needed {
                break;
            }
            let block = self.read_block(&mut compression)?;
            let block_start = position;
            position += block.len() as u64;

            while cursor < files.len() {
                let (index, want) = files[cursor];
                let (offset, size) = (self.members[index].offset, self.members[index].size);
                if offset >= position {
                    break;
                }
                // Opened on first touch; the parent directory is the caller's business.
                if open.is_none() {
                    let file = File::create(&want.destination).map_err(|error| {
                        io_error("write a toolchain file", &want.destination, &error)
                    })?;
                    open = Some(BufWriter::new(file));
                }
                let end = offset + size;
                let from = (offset.max(block_start) - block_start) as usize;
                let to = (end.min(position) - block_start) as usize;
                if let Some(writer) = open.as_mut() {
                    writer.write_all(&block[from..to]).map_err(|error| {
                        io_error("write a toolchain file", &want.destination, &error)
                    })?;
                }
                if end > position {
                    // Continues into the next block; the writer stays open.
                    break;
                }
                if let Some(mut writer) = open.take() {
                    writer.flush().map_err(|error| {
                        io_error("write a toolchain file", &want.destination, &error)
                    })?;
                }
                extracted.files += 1;
                extracted.bytes += size;
                cursor += 1;
            }
        }

        if cursor < files.len() {
            let (index, _) = files[cursor];
            return Err(unreadable(&format!(
                "{} in {} ends past its folder's data",
                self.members[index].name, self.label
            )));
        }
        Ok(extracted)
    }

    /// Read and decompress one `CFDATA`.
    fn read_block(&mut self, compression: &mut Compression) -> Result<Vec<u8>, ToolchainError> {
        let mut header = [0u8; DATA_HEADER_LENGTH];
        self.reader
            .read_exact(&mut header)
            .map_err(|error| stream(&self.label, "read a cabinet data block", &error))?;
        let compressed = le_u16(&header, 4)? as usize;
        let uncompressed = le_u16(&header, 6)? as usize;
        if self.data_reserve > 0 {
            self.reader
                .seek(SeekFrom::Current(self.data_reserve as i64))
                .map_err(|error| stream(&self.label, "read a cabinet data block", &error))?;
        }
        // A block declaring no uncompressed bytes continues in the next cabinet of the set.
        if uncompressed == 0 {
            return Err(ToolchainError::new(
                "The Windows SDK download could not be unpacked. Clear the installer's data \
                 folder and try again.",
                format!("a folder in {} continues into another cabinet", self.label),
            ));
        }
        let mut block = vec![0u8; compressed];
        self.reader
            .read_exact(&mut block)
            .map_err(|error| stream(&self.label, "read a cabinet data block", &error))?;
        compression.decompress(&block, uncompressed)
    }
}

/// A NUL-terminated name. Bounded, so a truncated cabinet cannot make this run away.
fn read_string(
    reader: &mut BufReader<File>,
    path: &Path,
    utf8: bool,
) -> Result<String, ToolchainError> {
    let mut bytes = Vec::with_capacity(64);
    for _ in 0..512 {
        let mut byte = [0u8; 1];
        reader
            .read_exact(&mut byte)
            .map_err(|error| io_error("read a name from a cabinet", path, &error))?;
        if byte[0] == 0 {
            return Ok(if utf8 {
                String::from_utf8_lossy(&bytes).into_owned()
            } else {
                // The code-page case. Every name in these cabinets is an MSI key, which is
                // ASCII; treating anything else as Latin-1 at least round-trips it.
                bytes.iter().map(|&byte| byte as char).collect()
            });
        }
        bytes.push(byte[0]);
    }
    Err(unreadable("a name in the cabinet is not terminated"))
}

fn le_u16(bytes: &[u8], offset: usize) -> Result<u16, ToolchainError> {
    let slice = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| unreadable("a cabinet structure is shorter than the format allows"))?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn le_u32(bytes: &[u8], offset: usize) -> Result<u32, ToolchainError> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| unreadable("a cabinet structure is shorter than the format allows"))?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn unreadable(detail: &str) -> ToolchainError {
    ToolchainError::new(
        "The Windows SDK download could not be unpacked. Clear the installer's data folder \
         and try again.",
        detail.to_string(),
    )
}

fn stream(label: &str, action: &str, error: &std::io::Error) -> ToolchainError {
    ToolchainError::new(
        "The Windows SDK download could not be unpacked. Clear the installer's data folder \
         and try again.",
        format!("failed to {action} in {label}: {error}"),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::test_fixtures::cabinet::build;

    fn write_cabinet(dir: &Path, files: &[(&str, &[u8])]) -> PathBuf {
        let path = dir.join("cab1.cab");
        std::fs::write(&path, build(files)).unwrap();
        path
    }

    fn extract_one(cabinet: &mut Cabinet, dir: &Path, name: &str) -> Vec<u8> {
        let destination = dir.join(format!("out-{name}"));
        cabinet
            .extract(&[Wanted {
                name,
                destination: destination.clone(),
            }])
            .unwrap();
        std::fs::read(destination).unwrap()
    }

    /// The property that matters: bytes out equal bytes in, for payloads big enough to span
    /// several of the format's 32 KB blocks.
    #[test]
    fn members_come_back_byte_for_byte() {
        let dir = tempfile::tempdir().unwrap();
        let compressible = vec![b'a'; 200_000];
        let incompressible: Vec<u8> = (0..200_000u32)
            .map(|index| index.wrapping_mul(2_654_435_761) as u8)
            .collect();
        let path = write_cabinet(
            dir.path(),
            &[
                ("windows.h", b"#include <winnt.h>\n"),
                ("big.lib", &compressible),
                ("noise.lib", &incompressible),
                ("last.h", b"/* last */\n"),
            ],
        );

        let mut cabinet = Cabinet::open(&path).unwrap();

        assert_eq!(
            extract_one(&mut cabinet, dir.path(), "windows.h"),
            b"#include <winnt.h>\n"
        );
        assert_eq!(
            extract_one(&mut cabinet, dir.path(), "big.lib"),
            compressible
        );
        assert_eq!(
            extract_one(&mut cabinet, dir.path(), "noise.lib"),
            incompressible
        );
        assert_eq!(
            extract_one(&mut cabinet, dir.path(), "last.h"),
            b"/* last */\n"
        );
    }

    /// The whole point of the rewrite: many files, one pass, all correct.
    #[test]
    fn many_members_are_extracted_in_a_single_pass() {
        let dir = tempfile::tempdir().unwrap();
        let contents: Vec<Vec<u8>> = (0..60u32)
            .map(|index| {
                (0..3_000u32)
                    .map(|byte| (byte.wrapping_add(index) % 251) as u8)
                    .collect()
            })
            .collect();
        let names: Vec<String> = (0..60).map(|index| format!("file{index:02}.h")).collect();
        let files: Vec<(&str, &[u8])> = names
            .iter()
            .zip(&contents)
            .map(|(name, content)| (name.as_str(), content.as_slice()))
            .collect();
        let path = write_cabinet(dir.path(), &files);

        let mut cabinet = Cabinet::open(&path).unwrap();
        // Deliberately out of order: the extractor must sort into folder order itself.
        let wanted: Vec<Wanted> = names
            .iter()
            .rev()
            .map(|name| Wanted {
                name,
                destination: dir.path().join(format!("out-{name}")),
            })
            .collect();
        let extracted = cabinet.extract(&wanted).unwrap();

        assert_eq!(extracted.files, 60);
        assert_eq!(extracted.bytes, 60 * 3_000);
        for (name, content) in names.iter().zip(&contents) {
            assert_eq!(
                &std::fs::read(dir.path().join(format!("out-{name}"))).unwrap(),
                content,
                "{name}"
            );
        }
    }

    /// The reference implementation is the oracle: whatever `cab` says a member contains,
    /// this reader must say the same.
    #[test]
    fn the_reference_implementation_agrees_member_for_member() {
        let dir = tempfile::tempdir().unwrap();
        let contents: Vec<Vec<u8>> = (0..12u32)
            .map(|index| {
                (0..40_000u32)
                    .map(|byte| byte.wrapping_mul(index + 1) as u8)
                    .collect()
            })
            .collect();
        let names: Vec<String> = (0..12).map(|index| format!("member{index:02}")).collect();
        let files: Vec<(&str, &[u8])> = names
            .iter()
            .zip(&contents)
            .map(|(name, content)| (name.as_str(), content.as_slice()))
            .collect();
        let path = write_cabinet(dir.path(), &files);

        let mut mine = Cabinet::open(&path).unwrap();
        let mut reference = cab::Cabinet::new(BufReader::new(File::open(&path).unwrap())).unwrap();

        for name in &names {
            let mut expected = Vec::new();
            reference
                .read_file(name)
                .unwrap()
                .read_to_end(&mut expected)
                .unwrap();
            assert_eq!(extract_one(&mut mine, dir.path(), name), expected, "{name}");
        }
    }

    /// Names are matched exactly, so a case-different name is *missing*, not a near miss.
    #[test]
    fn a_missing_member_names_itself_in_the_log_but_not_in_the_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_cabinet(dir.path(), &[("file1.h", b"x")]);
        let mut cabinet = Cabinet::open(&path).unwrap();

        let Err(error) = cabinet.extract(&[Wanted {
            name: "file2.h",
            destination: dir.path().join("out"),
        }]) else {
            panic!("that member is not in the cabinet");
        };

        assert!(error.detail().contains("file2.h"));
        assert!(!error.message().contains("file2.h"));
        // Nothing is written when a name does not resolve.
        assert!(!dir.path().join("out").exists());

        assert!(
            cabinet
                .extract(&[Wanted {
                    name: "FILE1.H",
                    destination: dir.path().join("out"),
                }])
                .is_err()
        );
    }

    #[test]
    fn something_that_is_not_a_cabinet_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cab1.cab");
        std::fs::write(
            &path,
            b"not a cabinet at all, but long enough to read a header",
        )
        .unwrap();

        let Err(error) = Cabinet::open(&path) else {
            panic!("a file that is not a cabinet must not open");
        };
        assert!(error.detail().contains("no MSCF signature"));
    }

    #[test]
    fn quantum_is_refused_with_a_sentence_rather_than_a_wrong_answer() {
        let Err(error) = Compression::parse(0x1472) else {
            panic!("Quantum is not supported");
        };
        assert!(error.message().contains("cannot read"));
        assert!(error.detail().contains("Quantum"));
    }

    #[test]
    fn compression_types_are_decoded_from_the_folder_header() {
        // `Lzx(MB2)` — what every cabinet on the real SDK image uses.
        let Ok(Compression::Lzx(_)) = Compression::parse(0x1503) else {
            panic!("0x1503 is LZX with a 2 MB window");
        };
        let Ok(Compression::MsZip(_)) = Compression::parse(0x0001) else {
            panic!("0x0001 is MSZIP");
        };
        let Ok(Compression::None) = Compression::parse(0x0000) else {
            panic!("0x0000 is uncompressed");
        };
    }
}
