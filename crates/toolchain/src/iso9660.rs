//! A read-only ISO9660 reader, enough to pull four known paths out of the SDK image.
//!
//! Hand-rolled rather than taken from crates.io. A dependency would have been legal — only
//! external *programs* are forbidden — but every ISO9660 crate on crates.io is
//! either explicitly incomplete, `no_std`-shaped for bootloaders, or unmaintained, and the
//! part of the format we need is small: descriptors start at a fixed sector, a volume
//! descriptor points at the root directory record, and directory records are a flat list of
//! fixed-layout structs. That is less code than vetting a dependency would be worth.
//!
//! What is supported: the Primary Volume Descriptor, Joliet supplementary descriptors
//! (preferred when present, because Microsoft's image stores the real mixed-case names
//! there), directories, and multi-extent files. What is not: Rock Ridge, El Torito, interleaved
//! files, and anything to do with writing.

use std::io::{Read, Seek, SeekFrom, Write};

use crate::error::{ToolchainError, missing_member, stream_error};

/// Every offset in the format is in units of this.
const LOGICAL_SECTOR_SIZE: u64 = 2048;

/// Volume descriptors start here — the first 16 sectors are the "system area" and are not
/// part of the filesystem.
const FIRST_DESCRIPTOR_SECTOR: u64 = 16;

/// Refuse to walk forever if the descriptor list has no terminator.
const MAX_DESCRIPTORS: usize = 64;

const DESCRIPTOR_PRIMARY: u8 = 1;
const DESCRIPTOR_SUPPLEMENTARY: u8 = 2;
const DESCRIPTOR_TERMINATOR: u8 = 255;

/// The root directory record sits inside every volume descriptor at this offset, and is
/// always exactly [`DIRECTORY_RECORD_ROOT_LEN`] bytes.
const ROOT_RECORD_OFFSET: usize = 156;
const DIRECTORY_RECORD_ROOT_LEN: usize = 34;

/// Joliet escape sequences: `%/@`, `%/C`, `%/E` for UCS-2 levels 1..3. They live at offset 88
/// of a supplementary descriptor.
const ESCAPE_SEQUENCES_OFFSET: usize = 88;
const JOLIET_ESCAPES: [&[u8]; 3] = [b"%/@", b"%/C", b"%/E"];

/// What `.` and `..` decode to. Neither is ever a name a caller can ask for.
const DOT: char = '\u{0}';
const DOT_DOT: char = '\u{1}';

/// File-flag bits we care about.
const FLAG_DIRECTORY: u8 = 0x02;
/// Set on every record of a multi-extent file except the last.
const FLAG_NOT_FINAL: u8 = 0x80;

/// How names are stored in the descriptor we chose to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameEncoding {
    /// Primary descriptor: uppercase ASCII, 8.3-ish, `;1` version suffixes.
    Ascii,
    /// Joliet supplementary descriptor: UCS-2, big-endian.
    Ucs2BigEndian,
}

/// One entry in a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The name as stored, minus the `;1` version suffix.
    pub name: String,
    pub is_directory: bool,
    /// Total size across all extents.
    pub size: u64,
    /// `(sector, byte length)` pairs, in order. More than one only for multi-extent files.
    extents: Vec<(u64, u64)>,
}

pub struct Iso9660<R> {
    reader: R,
    encoding: NameEncoding,
    root: Entry,
}

impl<R: Read + Seek> Iso9660<R> {
    /// Read the volume descriptors and settle on one to navigate by.
    ///
    /// Joliet wins when it is there. On Microsoft's SDK image the primary descriptor holds
    /// mangled 8.3 names (`WINSDK~1.MSI`) while Joliet holds `WinSDK_x86.msi`, so reading the
    /// primary would mean guessing at manglings — exactly what the extraction contract says
    /// not to do.
    pub fn open(mut reader: R) -> Result<Self, ToolchainError> {
        // Each candidate carries the encoding its names are stored in, so choosing a
        // descriptor and choosing how to decode it stay one decision.
        let mut primary: Option<(Entry, NameEncoding)> = None;
        let mut joliet: Option<(Entry, NameEncoding)> = None;

        for index in 0..MAX_DESCRIPTORS {
            let sector = FIRST_DESCRIPTOR_SECTOR + index as u64;
            let mut buffer = [0u8; LOGICAL_SECTOR_SIZE as usize];
            read_exact_at(&mut reader, sector * LOGICAL_SECTOR_SIZE, &mut buffer)
                .map_err(|error| error.context(format!("volume descriptor at sector {sector}")))?;

            if &buffer[1..6] != b"CD001" {
                return Err(not_an_iso(sector, &buffer[1..6]));
            }
            match buffer[0] {
                DESCRIPTOR_TERMINATOR => break,
                DESCRIPTOR_PRIMARY if primary.is_none() => {
                    let encoding = NameEncoding::Ascii;
                    primary = Some((root_record(&buffer, encoding)?, encoding));
                }
                DESCRIPTOR_SUPPLEMENTARY if joliet.is_none() && is_joliet(&buffer) => {
                    let encoding = NameEncoding::Ucs2BigEndian;
                    joliet = Some((root_record(&buffer, encoding)?, encoding));
                }
                _ => {}
            }
        }

        match joliet.or(primary) {
            Some((root, encoding)) => Ok(Self {
                reader,
                encoding,
                root,
            }),
            None => Err(ToolchainError::new(
                "The Windows SDK download is not a readable disc image.",
                "no primary or Joliet volume descriptor found in the first 64 descriptors",
            )),
        }
    }

    /// List one directory, given a forward-slashed path. `""` is the root.
    pub fn read_dir(&mut self, path: &str) -> Result<Vec<Entry>, ToolchainError> {
        let entry = self.resolve(path)?;
        if !entry.is_directory {
            return Err(missing_member(
                path,
                "the disc image (it is a file, not a folder)",
            ));
        }
        self.read_directory_entries(&entry)
    }

    /// Read a whole member into memory. For the MSIs, which are a few megabytes at most.
    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>, ToolchainError> {
        let entry = self.resolve(path)?;
        let mut bytes = Vec::with_capacity(entry.size as usize);
        self.copy_extents(&entry, &mut bytes)?;
        Ok(bytes)
    }

    /// Stream a member out. For the CABs, which run to hundreds of megabytes.
    pub fn copy_file_to(
        &mut self,
        path: &str,
        out: &mut impl Write,
    ) -> Result<u64, ToolchainError> {
        let entry = self.resolve(path)?;
        self.copy_extents(&entry, out)?;
        Ok(entry.size)
    }

    pub fn contains(&mut self, path: &str) -> bool {
        self.resolve(path).is_ok()
    }

    /// Walk a `/`-separated path from the root, matching case-insensitively.
    ///
    /// Case-insensitive because the same image can be navigated through either descriptor and
    /// the primary one uppercases everything; the paths in `docs/pinned-artifacts.md` are
    /// written in the Joliet casing.
    fn resolve(&mut self, path: &str) -> Result<Entry, ToolchainError> {
        let mut current = self.root.clone();
        for component in path.split('/').filter(|c| !c.is_empty() && *c != ".") {
            if !current.is_directory {
                return Err(missing_member(path, "the disc image"));
            }
            let children = self.read_directory_entries(&current)?;
            let found = children
                .into_iter()
                .find(|child| child.name.eq_ignore_ascii_case(component));
            match found {
                Some(child) => current = child,
                None => return Err(missing_member(path, "the disc image")),
            }
        }
        Ok(current)
    }

    fn read_directory_entries(&mut self, directory: &Entry) -> Result<Vec<Entry>, ToolchainError> {
        let mut raw = Vec::with_capacity(directory.size as usize);
        self.copy_extents(directory, &mut raw)?;

        let mut entries: Vec<RawRecord> = Vec::new();
        let mut offset = 0usize;
        while offset < raw.len() {
            let length = raw[offset] as usize;
            if length == 0 {
                // A record never straddles a sector: the rest of this one is padding.
                let next =
                    (offset / LOGICAL_SECTOR_SIZE as usize + 1) * LOGICAL_SECTOR_SIZE as usize;
                if next <= offset {
                    break;
                }
                offset = next;
                continue;
            }
            if offset + length > raw.len() {
                return Err(ToolchainError::new(
                    "The Windows SDK disc image is damaged.",
                    format!("directory record at offset {offset} runs past the end of its extent"),
                ));
            }
            let record = parse_record(&raw[offset..offset + length], self.encoding)?;
            offset += length;

            // `.` and `..` are stored as one-byte identifiers 0x00 and 0x01; `decode_name`
            // hands them back as those literal characters.
            if record.name.starts_with(DOT) || record.name.starts_with(DOT_DOT) {
                continue;
            }

            // Multi-extent files arrive as consecutive records sharing a name, all but the
            // last carrying FLAG_NOT_FINAL. Fold them into the entry already collected.
            match entries.last_mut() {
                Some(previous) if previous.continues && previous.name == record.name => {
                    previous.size += record.size;
                    previous.extents.extend(record.extents.iter().copied());
                    previous.continues = record.continues;
                }
                _ => entries.push(record),
            }
        }
        Ok(entries.into_iter().map(Entry::from).collect())
    }

    fn copy_extents(&mut self, entry: &Entry, out: &mut impl Write) -> Result<(), ToolchainError> {
        let mut buffer = vec![0u8; 256 * 1024];
        for &(sector, length) in &entry.extents {
            self.reader
                .seek(SeekFrom::Start(sector * LOGICAL_SECTOR_SIZE))
                .map_err(|error| stream_error("seek inside the disc image", &error))?;
            let mut remaining = length;
            while remaining > 0 {
                let want = remaining.min(buffer.len() as u64) as usize;
                self.reader
                    .read_exact(&mut buffer[..want])
                    .map_err(|error| stream_error("read from the disc image", &error))?;
                out.write_all(&buffer[..want])
                    .map_err(|error| stream_error("write an extracted file", &error))?;
                remaining -= want as u64;
            }
        }
        Ok(())
    }
}

/// A directory record as read off the disc, before multi-extent folding collapses it.
#[derive(Debug, Clone)]
struct RawRecord {
    name: String,
    is_directory: bool,
    size: u64,
    extents: Vec<(u64, u64)>,
    continues: bool,
}

impl From<RawRecord> for Entry {
    fn from(raw: RawRecord) -> Self {
        Entry {
            name: raw.name,
            is_directory: raw.is_directory,
            size: raw.size,
            extents: raw.extents,
        }
    }
}

fn parse_record(bytes: &[u8], encoding: NameEncoding) -> Result<RawRecord, ToolchainError> {
    if bytes.len() < 33 {
        return Err(ToolchainError::new(
            "The Windows SDK disc image is damaged.",
            format!("directory record is {} bytes, minimum is 33", bytes.len()),
        ));
    }
    let extended_attribute_sectors = bytes[1] as u64;
    let extent = u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]) as u64;
    let size = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]) as u64;
    let flags = bytes[25];
    let name_length = bytes[32] as usize;
    if 33 + name_length > bytes.len() {
        return Err(ToolchainError::new(
            "The Windows SDK disc image is damaged.",
            format!("directory record claims a {name_length}-byte name it does not contain"),
        ));
    }
    let name = decode_name(&bytes[33..33 + name_length], encoding);

    Ok(RawRecord {
        name,
        is_directory: flags & FLAG_DIRECTORY != 0,
        size,
        // The extended attribute record, when present, sits in front of the file data.
        extents: vec![(extent + extended_attribute_sectors, size)],
        continues: flags & FLAG_NOT_FINAL != 0,
    })
}

/// Turn an identifier into a name: decode, then drop the `;1` version suffix ISO9660 appends
/// to every file. Trailing `.` on extension-less names goes too — the format pads them.
fn decode_name(bytes: &[u8], encoding: NameEncoding) -> String {
    // `.` and `..` are the single bytes 0x00 and 0x01 in *both* encodings — they are not
    // UCS-2 under Joliet, which a general decoder would turn into an empty name.
    if bytes == [0] {
        return DOT.to_string();
    }
    if bytes == [1] {
        return DOT_DOT.to_string();
    }
    let decoded = match encoding {
        NameEncoding::Ascii => bytes.iter().map(|&b| b as char).collect::<String>(),
        NameEncoding::Ucs2BigEndian => {
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        }
    };
    let without_version = match decoded.rsplit_once(';') {
        Some((stem, version)) if version.chars().all(|c| c.is_ascii_digit()) => stem.to_string(),
        _ => decoded,
    };
    match without_version.strip_suffix('.') {
        Some(stripped) if !stripped.is_empty() => stripped.to_string(),
        _ => without_version,
    }
}

fn root_record(descriptor: &[u8], encoding: NameEncoding) -> Result<Entry, ToolchainError> {
    let slice = &descriptor[ROOT_RECORD_OFFSET..ROOT_RECORD_OFFSET + DIRECTORY_RECORD_ROOT_LEN];
    let mut root = Entry::from(parse_record(slice, encoding)?);
    // The root's identifier is the single byte 0x00; nothing should ever match on it.
    root.name = String::new();
    root.is_directory = true;
    Ok(root)
}

fn is_joliet(descriptor: &[u8]) -> bool {
    let escapes = &descriptor[ESCAPE_SEQUENCES_OFFSET..ESCAPE_SEQUENCES_OFFSET + 32];
    JOLIET_ESCAPES
        .iter()
        .any(|sequence| escapes.starts_with(sequence))
}

fn read_exact_at<R: Read + Seek>(
    reader: &mut R,
    offset: u64,
    buffer: &mut [u8],
) -> Result<(), ToolchainError> {
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|error| stream_error("seek inside the disc image", &error))?;
    reader
        .read_exact(buffer)
        .map_err(|error| stream_error("read from the disc image", &error))
}

fn not_an_iso(sector: u64, found: &[u8]) -> ToolchainError {
    ToolchainError::new(
        "The Windows SDK download is not a readable disc image. Clear the installer's data \
         folder and try again.",
        format!("sector {sector} should start with the CD001 signature, found {found:02x?}"),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::test_fixtures::iso::IsoBuilder;
    use std::io::Cursor;

    #[test]
    fn reads_a_nested_file_through_the_joliet_descriptor() {
        let image = IsoBuilder::new()
            .file("Setup/WinSDK/WinSDK_x86.msi", b"msi-bytes".to_vec())
            .file("Setup/WinSDK/cab1.cab", b"cab-bytes".to_vec())
            .build();

        let mut iso = Iso9660::open(Cursor::new(image)).unwrap();

        assert_eq!(
            iso.read_file("Setup/WinSDK/WinSDK_x86.msi").unwrap(),
            b"msi-bytes"
        );
        assert_eq!(
            iso.read_file("Setup/WinSDK/cab1.cab").unwrap(),
            b"cab-bytes"
        );
    }

    /// The paths in `docs/pinned-artifacts.md` are written in Joliet casing; the primary
    /// descriptor uppercases. Resolution must not care which one it ended up reading.
    #[test]
    fn path_lookup_ignores_case() {
        let image = IsoBuilder::new()
            .file("Setup/vc_stdx86/vc_stdx86.msi", b"crt".to_vec())
            .build();
        let mut iso = Iso9660::open(Cursor::new(image)).unwrap();

        assert_eq!(
            iso.read_file("SETUP/VC_STDX86/VC_STDX86.MSI").unwrap(),
            b"crt"
        );
        assert!(iso.contains("Setup/vc_stdx86/vc_stdx86.msi"));
    }

    #[test]
    fn a_missing_member_names_itself_in_the_log() {
        let image = IsoBuilder::new().file("Setup/a.txt", b"a".to_vec()).build();
        let mut iso = Iso9660::open(Cursor::new(image)).unwrap();

        let error = iso.read_file("Setup/WinSDK/WinSDK_x86.msi").unwrap_err();
        assert!(error.detail().contains("Setup/WinSDK/WinSDK_x86.msi"));
        // The sentence the user sees says nothing about disc image internals.
        assert!(!error.message().contains("Setup/WinSDK"));
    }

    #[test]
    fn a_file_spanning_several_sectors_comes_back_whole() {
        let payload: Vec<u8> = (0..(LOGICAL_SECTOR_SIZE as usize * 3 + 17))
            .map(|index| (index % 251) as u8)
            .collect();
        let image = IsoBuilder::new()
            .file("Setup/big.cab", payload.clone())
            .build();
        let mut iso = Iso9660::open(Cursor::new(image)).unwrap();

        let mut out = Vec::new();
        let copied = iso.copy_file_to("Setup/big.cab", &mut out).unwrap();

        assert_eq!(copied, payload.len() as u64);
        assert_eq!(out, payload);
    }

    #[test]
    fn a_directory_listing_omits_dot_and_dotdot() {
        let image = IsoBuilder::new()
            .file("Setup/WinSDK/one.msi", b"1".to_vec())
            .file("Setup/WinSDK/two.cab", b"2".to_vec())
            .build();
        let mut iso = Iso9660::open(Cursor::new(image)).unwrap();

        let mut names: Vec<String> = iso
            .read_dir("Setup/WinSDK")
            .unwrap()
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        names.sort();

        assert_eq!(names, vec!["one.msi".to_string(), "two.cab".to_string()]);
    }

    #[test]
    fn a_directory_with_more_entries_than_one_sector_holds_is_read_whole() {
        // Forces the directory extent past 2048 bytes, which is where the "record length 0
        // means skip to the next sector" rule starts to matter.
        let mut builder = IsoBuilder::new();
        for index in 0..40 {
            builder = builder.file(
                &format!("Setup/a-reasonably-long-member-name-{index:02}.cab"),
                vec![index as u8],
            );
        }
        let image = builder.build();
        let mut iso = Iso9660::open(Cursor::new(image)).unwrap();

        assert_eq!(iso.read_dir("Setup").unwrap().len(), 40);
        assert_eq!(
            iso.read_file("Setup/a-reasonably-long-member-name-39.cab")
                .unwrap(),
            vec![39u8]
        );
    }

    #[test]
    fn something_that_is_not_an_iso_is_refused_rather_than_misread() {
        let Err(error) = Iso9660::open(Cursor::new(vec![0u8; 64 * 1024])) else {
            panic!("64 KB of zeroes must not open as a disc image");
        };
        assert!(error.detail().contains("CD001"));
    }

    #[test]
    fn version_suffixes_and_padding_dots_are_stripped_from_names() {
        assert_eq!(decode_name(b"CAB1.CAB;1", NameEncoding::Ascii), "CAB1.CAB");
        assert_eq!(decode_name(b"README.;1", NameEncoding::Ascii), "README");
        assert_eq!(decode_name(b"SETUP", NameEncoding::Ascii), "SETUP");
        assert_eq!(
            decode_name(&[0, b'a', 0, b'b'], NameEncoding::Ucs2BigEndian),
            "ab"
        );
    }
}
