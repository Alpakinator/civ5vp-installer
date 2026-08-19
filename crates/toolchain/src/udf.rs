//! A read-only UDF (ECMA-167 / ISO-13346) reader, enough to pull four known paths out of the
//! SDK image.
//!
//! **This is the one that matters.** `docs/pinned-artifacts.md` and ADR-0001 describe the
//! Windows SDK image as ISO9660, and it is not: `GRMSDK_EN_DVD.iso` is a UDF disc whose
//! ISO9660 side is a two-file stub containing only a `README.TXT` that says
//!
//! > This disc contains a "UDF" file system and requires an operating system that supports
//! > the ISO-13346 "UDF" file system specification.
//!
//! Everything the bootstrap needs - `Setup/WinSDK`, `Setup/WinSDKBuild`, `Setup/vc_stdx86` -
//! lives on the UDF side. An ISO9660-only installer cannot extract this image at all.
//!
//! Hand-rolled for the same reason [`crate::iso9660`] is, and the walk is short: an anchor at
//! a fixed sector points at a descriptor sequence, that names the partition and the file set,
//! the file set names the root directory's File Entry, and directories are lists of File
//! Identifier Descriptors. What is supported: short and long allocation descriptors, embedded
//! (in-ICB) file data, and both File Entry flavours. What is not: multiple partitions,
//! virtual/sparable/metadata partition maps, named streams, and anything to do with writing.

use std::io::{Read, Seek, SeekFrom, Write};

use crate::error::{ToolchainError, missing_member, stream_error};

/// UDF puts its anchor at a fixed logical sector, and logical sectors are 2048 bytes on every
/// optical medium.
const SECTOR: u64 = 2048;

/// Where to look for an Anchor Volume Descriptor Pointer. 256 is the one the specification
/// requires; the others are where the mirror copies live on some writers.
const ANCHOR_SECTORS: [u64; 2] = [256, 512];

/// Descriptor tag identifiers (ECMA-167 3/7.2.1 and 4/7.2.1).
const TAG_ANCHOR: u16 = 2;
const TAG_PARTITION: u16 = 5;
const TAG_LOGICAL_VOLUME: u16 = 6;
const TAG_TERMINATING: u16 = 8;
const TAG_FILE_SET: u16 = 256;
const TAG_FILE_IDENTIFIER: u16 = 257;
const TAG_ALLOCATION_EXTENT: u16 = 258;
const TAG_FILE_ENTRY: u16 = 261;
const TAG_EXTENDED_FILE_ENTRY: u16 = 266;

/// The largest logical block size this reader will accept.
///
/// Real optical media use 512..=32768; the value is read straight out of the image and then
/// used as an allocation length for every directory entry, so an unbounded one is an abort
/// waiting to happen rather than an error a user can be shown.
const MAX_BLOCK_SIZE: u64 = 32 * 1024;

/// Fixed header sizes, up to but not including the extended-attribute area.
const FILE_ENTRY_HEADER: usize = 176;
const EXTENDED_FILE_ENTRY_HEADER: usize = 216;

/// `ICBTag.FileType` values we distinguish.
const FILE_TYPE_DIRECTORY: u8 = 4;

/// `ICBTag.Flags & 0x07`: how the allocation descriptor area is to be read.
const AD_SHORT: u16 = 0;
const AD_LONG: u16 = 1;
const AD_EXTENDED: u16 = 2;
const AD_EMBEDDED: u16 = 3;

/// Extent types, in the top two bits of an allocation descriptor's length.
const EXTENT_RECORDED: u32 = 0;
const EXTENT_CONTINUATION: u32 = 3;

/// `FileIdentifierDescriptor.FileCharacteristics` bits. The "is a directory" bit is
/// deliberately not among them: the File Entry the descriptor points at says so too, and
/// believing the directory listing over the entry itself is how a mismatch becomes a silent
/// misread.
const FID_DELETED: u8 = 0x04;
const FID_PARENT: u8 = 0x08;

/// Refuse to chase an allocation-descriptor chain forever.
const MAX_AD_CONTINUATIONS: usize = 64;
/// Refuse to walk a descriptor sequence forever.
const MAX_VOLUME_DESCRIPTORS: usize = 64;

/// One name in a directory, before its File Entry has been read.
#[derive(Debug, Clone)]
struct FileIdentifier {
    name: String,
    /// Logical block of the File Entry, relative to the partition.
    icb_block: u64,
}

/// A file or directory, with everything needed to read its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_directory: bool,
    pub size: u64,
    /// Absolute `(byte offset, length)` pairs, in order.
    extents: Vec<(u64, u64)>,
    /// Set when the data is stored inside the File Entry itself, which UDF allows for
    /// anything small enough - including, on this image, the root directory.
    inline: Option<Vec<u8>>,
}

pub struct Udf<R> {
    reader: R,
    /// First logical block of the partition, in sectors from the start of the image.
    partition_start: u64,
    block_size: u64,
    root: Entry,
}

/// Whether this looks like a UDF volume, without committing to reading it as one.
///
/// Cheap: two sector reads. [`crate::disc`] uses it to choose a reader, so it has to be able
/// to say "no" about a plain ISO9660 image without disturbing anything.
pub fn has_anchor<R: Read + Seek>(reader: &mut R) -> bool {
    ANCHOR_SECTORS.iter().any(|sector| {
        let mut buffer = [0u8; 32];
        read_at(reader, sector * SECTOR, &mut buffer).is_ok() && tag_id(&buffer) == TAG_ANCHOR
    })
}

impl<R: Read + Seek> Udf<R> {
    /// Walk anchor → volume descriptor sequence → file set → root directory.
    pub fn open(mut reader: R) -> Result<Self, ToolchainError> {
        let anchor = find_anchor(&mut reader)?;
        // The anchor's first field is an `extent_ad`: the length in bytes and the location of
        // the Main Volume Descriptor Sequence.
        let sequence_length = le_u32(&anchor, 16)? as u64;
        let sequence_start = le_u32(&anchor, 20)? as u64;

        let mut block_size = SECTOR;
        let mut partition_start: Option<u64> = None;
        let mut file_set_block: Option<u64> = None;

        let blocks = (sequence_length / SECTOR).min(MAX_VOLUME_DESCRIPTORS as u64);
        for index in 0..blocks {
            let mut buffer = vec![0u8; SECTOR as usize];
            read_at(&mut reader, (sequence_start + index) * SECTOR, &mut buffer)
                .map_err(|error| error.context("volume descriptor sequence"))?;
            match tag_id(&buffer) {
                TAG_TERMINATING | 0 => break,
                TAG_PARTITION => {
                    partition_start = Some(le_u32(&buffer, 188)? as u64);
                }
                TAG_LOGICAL_VOLUME => {
                    block_size = le_u32(&buffer, 212)? as u64;
                    // `LogicalVolumeContentsUse` is a long_ad pointing at the File Set
                    // Descriptor; its block number sits four bytes into the descriptor.
                    file_set_block = Some(le_u32(&buffer, 248 + 4)? as u64);
                }
                _ => {}
            }
        }

        let (Some(partition_start), Some(file_set_block)) = (partition_start, file_set_block)
        else {
            return Err(unreadable(
                "the volume descriptor sequence has no partition or no file set",
            ));
        };
        // Bounded, not merely non-zero. `block_size` comes straight out of the image and is
        // then used as an allocation length for every directory entry read, so a descriptor
        // claiming 4 GiB would abort the process on the allocator rather than return an error
        // a user can be shown. Real UDF block sizes are 512..=32768.
        if block_size == 0 || block_size > MAX_BLOCK_SIZE {
            return Err(unreadable(&format!(
                "the logical volume declares a block size of {block_size}, which is not a size \
                 any real disc uses"
            )));
        }

        let mut volume = Self {
            reader,
            partition_start,
            block_size,
            root: Entry {
                name: String::new(),
                is_directory: true,
                size: 0,
                extents: Vec::new(),
                inline: None,
            },
        };

        let mut file_set = vec![0u8; volume.block_size as usize];
        let offset = volume.block_offset(file_set_block);
        read_at(&mut volume.reader, offset, &mut file_set)
            .map_err(|error| error.context("file set descriptor"))?;
        if tag_id(&file_set) != TAG_FILE_SET {
            return Err(unreadable(
                "the file set descriptor is not where the volume says",
            ));
        }
        // `RootDirectoryICB` is a long_ad at offset 400.
        let root_block = le_u32(&file_set, 400 + 4)? as u64;
        volume.root = volume.read_entry(root_block, String::new())?;
        if !volume.root.is_directory {
            return Err(unreadable("the root directory is not a directory"));
        }
        Ok(volume)
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
        let identifiers = self.read_identifiers(&entry)?;
        identifiers
            .into_iter()
            .map(|identifier| self.read_entry(identifier.icb_block, identifier.name))
            .collect()
    }

    /// Read a whole member into memory. For the MSIs, which are a few megabytes at most.
    /// Read a whole member into memory.
    ///
    /// Test-only: the bootstrap streams members out with `copy_file_to`, so a 43 MB cabinet
    /// is never held in memory. This is what the image-inspection tools read with.
    #[cfg(test)]
    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>, ToolchainError> {
        let entry = self.resolve(path)?;
        // Deliberately not `with_capacity(entry.size)`: that length is whatever the image says
        // it is, and a file entry claiming 2^60 bytes would abort on the allocator before a
        // single byte was read. Growing as the bytes actually arrive costs a few reallocations
        // on a several-megabyte MSI and cannot be talked into a huge one.
        let mut bytes = Vec::new();
        self.copy_entry(&entry, &mut bytes)?;
        Ok(bytes)
    }

    /// Stream a member out. For the CABs, which run to hundreds of megabytes.
    pub fn copy_file_to(
        &mut self,
        path: &str,
        out: &mut impl Write,
    ) -> Result<u64, ToolchainError> {
        let entry = self.resolve(path)?;
        self.copy_entry(&entry, out)?;
        Ok(entry.size)
    }

    pub fn contains(&mut self, path: &str) -> bool {
        self.resolve(path).is_ok()
    }

    /// Where `path`'s bytes actually lie in the image: absolute `(offset, length)` pairs, in
    /// order.
    ///
    /// Test-only, because at runtime the offsets come from the pin rather than from the
    /// image: this is the tool that *measured* the pin (see `describe_the_pinned_members`),
    /// and the check that each member really is one run of bytes.
    ///
    /// A file small enough to live inside its own File Entry has no extents at all, which is
    /// why the answer can be empty.
    #[cfg(test)]
    pub fn extents(&mut self, path: &str) -> Result<Vec<(u64, u64)>, ToolchainError> {
        Ok(self.resolve(path)?.extents)
    }

    /// Walk a `/`-separated path from the root, matching case-insensitively.
    ///
    /// Only the matched child's File Entry is read, not every sibling's - `Setup/` has a few
    /// dozen entries and each one costs a seek.
    fn resolve(&mut self, path: &str) -> Result<Entry, ToolchainError> {
        let mut current = self.root.clone();
        for component in path.split('/').filter(|c| !c.is_empty() && *c != ".") {
            if !current.is_directory {
                return Err(missing_member(path, "the disc image"));
            }
            let identifiers = self.read_identifiers(&current)?;
            let found = identifiers
                .into_iter()
                .find(|identifier| identifier.name.eq_ignore_ascii_case(component));
            match found {
                Some(identifier) => {
                    current = self.read_entry(identifier.icb_block, identifier.name)?;
                }
                None => return Err(missing_member(path, "the disc image")),
            }
        }
        Ok(current)
    }

    /// Read the File Entry at `icb_block` and turn it into an [`Entry`].
    fn read_entry(&mut self, icb_block: u64, name: String) -> Result<Entry, ToolchainError> {
        let mut buffer = vec![0u8; self.block_size as usize];
        let offset = self.block_offset(icb_block);
        read_at(&mut self.reader, offset, &mut buffer)
            .map_err(|error| error.context(format!("file entry at block {icb_block}")))?;

        let header = match tag_id(&buffer) {
            TAG_FILE_ENTRY => FILE_ENTRY_HEADER,
            TAG_EXTENDED_FILE_ENTRY => EXTENDED_FILE_ENTRY_HEADER,
            other => {
                return Err(unreadable(&format!(
                    "block {icb_block} should hold a file entry, found descriptor {other}"
                )));
            }
        };

        // The ICB tag sits at offset 16; `FileType` is 11 bytes into it and `Flags` 18.
        let file_type = *buffer.get(16 + 11).ok_or_else(short_entry)?;
        let flags = le_u16(&buffer, 16 + 18)?;
        let size = le_u64(&buffer, 56)?;
        let extended_attributes = le_u32(&buffer, header - 8)? as usize;
        let descriptors_length = le_u32(&buffer, header - 4)? as usize;

        // `checked_add`, because on a 32-bit target both of these are `u32`-derived and their
        // sum can wrap - which would slip past the bound below and panic on the slice instead.
        let (Some(start), Some(end)) = (
            header.checked_add(extended_attributes),
            header
                .checked_add(extended_attributes)
                .and_then(|s| s.checked_add(descriptors_length)),
        ) else {
            return Err(unreadable(&format!(
                "file entry at block {icb_block} declares lengths that do not add up"
            )));
        };
        if end > buffer.len() {
            return Err(unreadable(&format!(
                "file entry at block {icb_block} claims {descriptors_length} bytes of \
                 allocation descriptors that do not fit in a block"
            )));
        }
        let area = buffer[start..end].to_vec();

        let (extents, inline) = match flags & 0x07 {
            AD_EMBEDDED => {
                let mut data = area;
                data.truncate(size as usize);
                (Vec::new(), Some(data))
            }
            kind @ (AD_SHORT | AD_LONG | AD_EXTENDED) => (self.read_extents(area, kind)?, None),
            other => {
                return Err(unreadable(&format!(
                    "file entry at block {icb_block} uses allocation descriptor type {other}"
                )));
            }
        };

        Ok(Entry {
            name,
            is_directory: file_type == FILE_TYPE_DIRECTORY,
            size,
            extents,
            inline,
        })
    }

    /// Parse an allocation descriptor area into absolute byte extents, following any
    /// continuation descriptors.
    fn read_extents(
        &mut self,
        mut area: Vec<u8>,
        kind: u16,
    ) -> Result<Vec<(u64, u64)>, ToolchainError> {
        let stride = match kind {
            AD_SHORT => 8usize,
            AD_LONG => 16,
            _ => 20,
        };
        // A short_ad's block number is its second word; long_ad and ext_ad put a whole
        // `lb_addr` there, whose first word is still the block number.
        let block_field = 4usize;

        let mut extents = Vec::new();
        for _ in 0..MAX_AD_CONTINUATIONS {
            let mut continuation: Option<u64> = None;
            let mut offset = 0usize;
            while offset + stride <= area.len() {
                let raw_length = le_u32(&area, offset)?;
                let block = le_u32(&area, offset + block_field)? as u64;
                let extent_type = raw_length >> 30;
                let length = u64::from(raw_length & 0x3FFF_FFFF);
                offset += stride;

                if length == 0 {
                    continue;
                }
                match extent_type {
                    EXTENT_RECORDED => extents.push((self.block_offset(block), length)),
                    EXTENT_CONTINUATION => {
                        continuation = Some(block);
                        break;
                    }
                    // Allocated-but-unrecorded and unallocated extents read as zeroes; the
                    // members we care about are never sparse, and silently shortening a file
                    // would be worse than saying so.
                    _ => {
                        return Err(unreadable(
                            "a member of the disc image is stored sparsely, which the \
                             installer cannot read",
                        ));
                    }
                }
            }

            let Some(block) = continuation else {
                return Ok(extents);
            };
            // The continuation block starts with an Allocation Extent Descriptor whose last
            // header word is the length of the descriptors that follow it.
            let mut buffer = vec![0u8; self.block_size as usize];
            let offset = self.block_offset(block);
            read_at(&mut self.reader, offset, &mut buffer)
                .map_err(|error| error.context("allocation extent descriptor"))?;
            if tag_id(&buffer) != TAG_ALLOCATION_EXTENT {
                return Err(unreadable(
                    "an allocation descriptor chain points at something that is not one",
                ));
            }
            let length = le_u32(&buffer, 20)? as usize;
            let end = (24 + length).min(buffer.len());
            area = buffer[24..end].to_vec();
        }
        Err(unreadable(
            "an allocation descriptor chain is longer than any real file needs",
        ))
    }

    /// Parse a directory's File Identifier Descriptors.
    fn read_identifiers(
        &mut self,
        directory: &Entry,
    ) -> Result<Vec<FileIdentifier>, ToolchainError> {
        let mut data = Vec::new();
        self.copy_entry(directory, &mut data)?;

        let mut identifiers = Vec::new();
        let mut offset = 0usize;
        while offset + 38 <= data.len() {
            if tag_id(&data[offset..]) != TAG_FILE_IDENTIFIER {
                break;
            }
            let characteristics = *data.get(offset + 18).ok_or_else(short_entry)?;
            let name_length = *data.get(offset + 19).ok_or_else(short_entry)? as usize;
            // The ICB is a long_ad at offset 20; its block number is four bytes in.
            let icb_block = le_u32(&data, offset + 24)? as u64;
            let implementation_use = le_u16(&data, offset + 36)? as usize;

            let name_start = offset + 38 + implementation_use;
            let name_end = name_start + name_length;
            if name_end > data.len() {
                break;
            }
            // Every descriptor is padded to a four-byte boundary.
            offset = name_end.div_ceil(4) * 4;

            // The parent entry has no name and is not a child; deleted ones are not there.
            if characteristics & FID_PARENT != 0 || characteristics & FID_DELETED != 0 {
                continue;
            }
            identifiers.push(FileIdentifier {
                name: decode_identifier(&data[name_start..name_end]),
                icb_block,
            });
        }
        Ok(identifiers)
    }

    fn copy_entry(&mut self, entry: &Entry, out: &mut impl Write) -> Result<(), ToolchainError> {
        if let Some(inline) = &entry.inline {
            return out
                .write_all(inline)
                .map_err(|error| stream_error("write an extracted file", &error));
        }
        let mut buffer = vec![0u8; 256 * 1024];
        for &(offset, length) in &entry.extents {
            self.reader
                .seek(SeekFrom::Start(offset))
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

    /// Logical block within the partition → byte offset in the image.
    fn block_offset(&self, block: u64) -> u64 {
        (self.partition_start + block) * self.block_size
    }
}

fn find_anchor<R: Read + Seek>(reader: &mut R) -> Result<Vec<u8>, ToolchainError> {
    for sector in ANCHOR_SECTORS {
        let mut buffer = vec![0u8; SECTOR as usize];
        if read_at(reader, sector * SECTOR, &mut buffer).is_ok() && tag_id(&buffer) == TAG_ANCHOR {
            return Ok(buffer);
        }
    }
    Err(unreadable("no UDF anchor at sector 256 or 512"))
}

/// A File Identifier is a `d-string`: a compression byte, then either 8-bit or UTF-16BE
/// characters.
fn decode_identifier(bytes: &[u8]) -> String {
    match bytes.split_first() {
        Some((8, rest)) => rest.iter().map(|&byte| byte as char).collect(),
        Some((16, rest)) => {
            let units: Vec<u16> = rest
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => String::new(),
    }
}

fn tag_id(bytes: &[u8]) -> u16 {
    match (bytes.first(), bytes.get(1)) {
        (Some(low), Some(high)) => u16::from_le_bytes([*low, *high]),
        _ => 0,
    }
}

fn le_u16(bytes: &[u8], offset: usize) -> Result<u16, ToolchainError> {
    let slice = bytes.get(offset..offset + 2).ok_or_else(short_entry)?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn le_u32(bytes: &[u8], offset: usize) -> Result<u32, ToolchainError> {
    let slice = bytes.get(offset..offset + 4).ok_or_else(short_entry)?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn le_u64(bytes: &[u8], offset: usize) -> Result<u64, ToolchainError> {
    let slice = bytes.get(offset..offset + 8).ok_or_else(short_entry)?;
    let mut value = [0u8; 8];
    value.copy_from_slice(slice);
    Ok(u64::from_le_bytes(value))
}

fn short_entry() -> ToolchainError {
    unreadable("a descriptor in the disc image is shorter than the format allows")
}

fn read_at<R: Read + Seek>(
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

fn unreadable(detail: &str) -> ToolchainError {
    ToolchainError::new(
        "The Windows SDK download is not a readable disc image. Clear the installer's data \
         folder and try again.",
        detail.to_string(),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::test_fixtures::udf::UdfBuilder;
    use std::io::Cursor;

    #[test]
    fn reads_a_nested_file() {
        let image = UdfBuilder::new()
            .file("Setup/WinSDK/WinSDK_x86.msi", b"msi-bytes".to_vec())
            .file("Setup/WinSDK/cab1.cab", b"cab-bytes".to_vec())
            .build();

        let mut udf = Udf::open(Cursor::new(image)).unwrap();

        assert_eq!(
            udf.read_file("Setup/WinSDK/WinSDK_x86.msi").unwrap(),
            b"msi-bytes"
        );
        assert_eq!(
            udf.read_file("Setup/WinSDK/cab1.cab").unwrap(),
            b"cab-bytes"
        );
    }

    #[test]
    fn an_anchor_is_what_tells_a_udf_image_from_an_iso9660_one() {
        let udf = UdfBuilder::new().file("a.txt", b"a".to_vec()).build();
        assert!(has_anchor(&mut Cursor::new(udf)));

        let iso = crate::test_fixtures::iso::IsoBuilder::new()
            .file("a.txt", b"a".to_vec())
            .build();
        assert!(!has_anchor(&mut Cursor::new(iso)));
    }

    #[test]
    fn path_lookup_ignores_case() {
        let image = UdfBuilder::new()
            .file("Setup/vc_stdx86/vc_stdx86.msi", b"crt".to_vec())
            .build();
        let mut udf = Udf::open(Cursor::new(image)).unwrap();

        assert_eq!(
            udf.read_file("SETUP/VC_STDX86/VC_STDX86.MSI").unwrap(),
            b"crt"
        );
        assert!(udf.contains("Setup/vc_stdx86/vc_stdx86.msi"));
    }

    /// The real image stores its root directory inside the File Entry rather than in an
    /// extent of its own, so this is not an exotic corner.
    #[test]
    fn a_directory_stored_inside_its_file_entry_is_read() {
        let image = UdfBuilder::new()
            .file("one.msi", b"1".to_vec())
            .file("two.cab", b"22".to_vec())
            .build();
        let mut udf = Udf::open(Cursor::new(image)).unwrap();

        let mut entries: Vec<(String, u64)> = udf
            .read_dir("")
            .unwrap()
            .into_iter()
            .map(|entry| (entry.name, entry.size))
            .collect();
        entries.sort();

        assert_eq!(
            entries,
            vec![("one.msi".to_string(), 1), ("two.cab".to_string(), 2)]
        );
    }

    #[test]
    fn a_file_spanning_several_blocks_comes_back_whole() {
        let payload: Vec<u8> = (0..(SECTOR as usize * 3 + 17))
            .map(|index| (index % 251) as u8)
            .collect();
        let image = UdfBuilder::new()
            .file("Setup/big.cab", payload.clone())
            .build();
        let mut udf = Udf::open(Cursor::new(image)).unwrap();

        let mut out = Vec::new();
        let copied = udf.copy_file_to("Setup/big.cab", &mut out).unwrap();

        assert_eq!(copied, payload.len() as u64);
        assert_eq!(out, payload);
    }

    /// A directory with more children than fit inside its File Entry has to be spilled into
    /// an extent, which is the other half of the reader.
    #[test]
    fn a_directory_too_big_to_embed_is_read_from_its_extent() {
        let mut builder = UdfBuilder::new();
        for index in 0..40 {
            builder = builder.file(
                &format!("Setup/a-reasonably-long-member-name-{index:02}.cab"),
                vec![index as u8],
            );
        }
        let mut udf = Udf::open(Cursor::new(builder.build())).unwrap();

        assert_eq!(udf.read_dir("Setup").unwrap().len(), 40);
        assert_eq!(
            udf.read_file("Setup/a-reasonably-long-member-name-39.cab")
                .unwrap(),
            vec![39u8]
        );
    }

    #[test]
    fn a_missing_member_names_itself_in_the_log() {
        let image = UdfBuilder::new().file("Setup/a.txt", b"a".to_vec()).build();
        let mut udf = Udf::open(Cursor::new(image)).unwrap();

        let Err(error) = udf.read_file("Setup/WinSDK/WinSDK_x86.msi") else {
            panic!("that member is not there");
        };
        assert!(error.detail().contains("Setup/WinSDK/WinSDK_x86.msi"));
        assert!(!error.message().contains("Setup/WinSDK"));
    }

    #[test]
    fn something_that_is_not_a_udf_volume_is_refused_rather_than_misread() {
        let Err(error) = Udf::open(Cursor::new(vec![0u8; 2 * 1024 * 1024])) else {
            panic!("2 MB of zeroes must not open as a UDF volume");
        };
        assert!(error.detail().contains("anchor"));
    }

    #[test]
    fn identifiers_are_decoded_in_both_the_formats_udf_allows() {
        assert_eq!(
            decode_identifier(&[8, b'S', b'e', b't', b'u', b'p']),
            "Setup"
        );
        assert_eq!(decode_identifier(&[16, 0, b'a', 0, b'b']), "ab");
        assert_eq!(decode_identifier(&[]), "");
    }
}
