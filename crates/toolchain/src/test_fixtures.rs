//! Synthetic archives, built byte by byte, so the readers can be tested without the 1.45 GiB
//! download (rule 13).
//!
//! Everything here is `#[cfg(test)]`: it is compiled into the test binary only and is not
//! part of the crate's API. The CAB and MSI builders are thin wrappers over the writer halves
//! of the `cab` and `msi` crates, which is what makes those fixtures worth trusting — they
//! are produced by the same format implementations the bootstrap reads with. The ISO builder
//! has no such counterpart, so it writes the descriptors and directory records itself.

/// A minimal ISO9660 writer: a Primary Volume Descriptor with uppercased ASCII names, a
/// Joliet supplementary descriptor with UCS-2 names, and both pointing at the same file
/// extents.
///
/// The two trees really are different, which is the point. The reader prefers Joliet and
/// decodes it as UCS-2, so a fixture that wrote ASCII names under a Joliet escape would be
/// testing the descriptor choice and the name decoding against each other rather than
/// against the format.
pub mod iso {
    use std::collections::BTreeMap;

    const SECTOR: usize = 2048;

    /// How a directory record's identifier is encoded.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Encoding {
        /// Primary descriptor: ASCII, uppercased the way a real mastering tool does.
        Ascii,
        /// Joliet supplementary descriptor: UCS-2, big-endian, original case.
        Ucs2,
    }

    /// A directory being assembled.
    #[derive(Default)]
    struct Dir {
        dirs: BTreeMap<String, Dir>,
        files: BTreeMap<String, Vec<u8>>,
    }

    /// Collects paths, then lays them out into a disc image.
    #[derive(Default)]
    pub struct IsoBuilder {
        root: Dir,
    }

    impl IsoBuilder {
        pub fn new() -> Self {
            Self::default()
        }

        /// Add a file at a `/`-separated path, creating parent directories as needed.
        pub fn file(mut self, path: &str, content: Vec<u8>) -> Self {
            let mut components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
            let Some(name) = components.pop() else {
                return self;
            };
            let mut current = &mut self.root;
            for component in components {
                current = current.dirs.entry(component.to_string()).or_default();
            }
            current.files.insert(name.to_string(), content);
            self
        }

        pub fn build(self) -> Vec<u8> {
            // Sector 16 is the PVD, 17 the Joliet SVD, 18 the terminator, 19 onwards data.
            let mut next_sector = 19u32;
            let mut writes: Vec<(u32, Vec<u8>)> = Vec::new();

            // File contents are laid out once and referenced by both directory trees.
            let mut file_sectors: BTreeMap<String, Extent> = BTreeMap::new();
            place_files(
                &self.root,
                "",
                &mut file_sectors,
                &mut writes,
                &mut next_sector,
            );

            let ascii_root = place_tree(
                &self.root,
                Encoding::Ascii,
                "",
                &file_sectors,
                &mut writes,
                &mut next_sector,
            );
            let joliet_root = place_tree(
                &self.root,
                Encoding::Ucs2,
                "",
                &file_sectors,
                &mut writes,
                &mut next_sector,
            );

            let mut image = vec![0u8; next_sector as usize * SECTOR];
            for (sector, bytes) in writes {
                let start = sector as usize * SECTOR;
                image[start..start + bytes.len()].copy_from_slice(&bytes);
            }

            write_descriptor(&mut image, 16, 1, ascii_root, next_sector, false);
            write_descriptor(&mut image, 17, 2, joliet_root, next_sector, true);
            image[18 * SECTOR] = 255;
            image[18 * SECTOR + 1..18 * SECTOR + 6].copy_from_slice(b"CD001");
            image[18 * SECTOR + 6] = 1;
            image
        }
    }

    /// `(sector, byte length)`.
    type Extent = (u32, u32);

    struct Record {
        name: String,
        extent: Extent,
        is_directory: bool,
    }

    /// Give every file in the tree a sector range, keyed by its full path.
    fn place_files(
        dir: &Dir,
        prefix: &str,
        out: &mut BTreeMap<String, Extent>,
        writes: &mut Vec<(u32, Vec<u8>)>,
        next: &mut u32,
    ) {
        for (name, content) in &dir.files {
            let sectors = content.len().div_ceil(SECTOR).max(1) as u32;
            let sector = *next;
            *next += sectors;
            writes.push((sector, content.clone()));
            out.insert(format!("{prefix}{name}"), (sector, content.len() as u32));
        }
        for (name, child) in &dir.dirs {
            place_files(child, &format!("{prefix}{name}/"), out, writes, next);
        }
    }

    /// Lay out one directory tree in one encoding, post-order so a parent's records can name
    /// sectors its children already own.
    fn place_tree(
        dir: &Dir,
        encoding: Encoding,
        prefix: &str,
        file_sectors: &BTreeMap<String, Extent>,
        writes: &mut Vec<(u32, Vec<u8>)>,
        next: &mut u32,
    ) -> Extent {
        // `.` and `..` come first, as the format requires. Their extents are never followed —
        // the reader skips both — so zeroes are enough.
        let mut records = vec![
            Record {
                name: DOT.to_string(),
                extent: (0, 0),
                is_directory: true,
            },
            Record {
                name: DOT_DOT.to_string(),
                extent: (0, 0),
                is_directory: true,
            },
        ];

        for name in dir.files.keys() {
            let extent = file_sectors
                .get(&format!("{prefix}{name}"))
                .copied()
                .unwrap_or((0, 0));
            records.push(Record {
                name: name.clone(),
                extent,
                is_directory: false,
            });
        }
        for (name, child) in &dir.dirs {
            let extent = place_tree(
                child,
                encoding,
                &format!("{prefix}{name}/"),
                file_sectors,
                writes,
                next,
            );
            records.push(Record {
                name: name.clone(),
                extent,
                is_directory: true,
            });
        }

        let mut bytes = Vec::new();
        for record in &records {
            push_record(&mut bytes, record, encoding);
        }
        let sectors = bytes.len().div_ceil(SECTOR).max(1) as u32;
        bytes.resize(sectors as usize * SECTOR, 0);
        let sector = *next;
        *next += sectors;
        writes.push((sector, bytes));
        (sector, sectors * SECTOR as u32)
    }

    /// The identifiers ISO9660 uses for `.` and `..`.
    const DOT: char = '\u{0}';
    const DOT_DOT: char = '\u{1}';

    fn encode_identifier(record: &Record, encoding: Encoding) -> Vec<u8> {
        if record.name.len() == 1 {
            if record.name.starts_with(DOT) {
                return vec![0];
            }
            if record.name.starts_with(DOT_DOT) {
                return vec![1];
            }
        }
        // Files carry the `;1` version suffix a real image writes; directories do not.
        let text = if record.is_directory {
            record.name.clone()
        } else {
            format!("{};1", record.name)
        };
        match encoding {
            Encoding::Ascii => text.to_ascii_uppercase().into_bytes(),
            Encoding::Ucs2 => text
                .encode_utf16()
                .flat_map(|unit| unit.to_be_bytes())
                .collect(),
        }
    }

    fn push_record(out: &mut Vec<u8>, record: &Record, encoding: Encoding) {
        let name = encode_identifier(record, encoding);
        let mut length = 33 + name.len();
        if length % 2 == 1 {
            length += 1;
        }

        // A record may not straddle a sector boundary: pad out to the next one instead.
        let position = out.len() % SECTOR;
        if position + length > SECTOR {
            out.resize(out.len() + (SECTOR - position), 0);
        }

        let (sector, size) = record.extent;
        let start = out.len();
        out.resize(start + length, 0);
        out[start] = length as u8;
        out[start + 2..start + 6].copy_from_slice(&sector.to_le_bytes());
        out[start + 6..start + 10].copy_from_slice(&sector.to_be_bytes());
        out[start + 10..start + 14].copy_from_slice(&size.to_le_bytes());
        out[start + 14..start + 18].copy_from_slice(&size.to_be_bytes());
        out[start + 25] = if record.is_directory { 0x02 } else { 0x00 };
        out[start + 32] = name.len() as u8;
        out[start + 33..start + 33 + name.len()].copy_from_slice(&name);
    }

    fn write_descriptor(
        image: &mut [u8],
        sector: usize,
        descriptor_type: u8,
        root: Extent,
        total_sectors: u32,
        joliet: bool,
    ) {
        let base = sector * SECTOR;
        image[base] = descriptor_type;
        image[base + 1..base + 6].copy_from_slice(b"CD001");
        image[base + 6] = 1;
        image[base + 80..base + 84].copy_from_slice(&total_sectors.to_le_bytes());
        image[base + 84..base + 88].copy_from_slice(&total_sectors.to_be_bytes());
        if joliet {
            image[base + 88..base + 91].copy_from_slice(b"%/E");
        }
        image[base + 128..base + 130].copy_from_slice(&(SECTOR as u16).to_le_bytes());

        // The root directory record, inline at offset 156.
        let record = base + 156;
        image[record] = 34;
        image[record + 2..record + 6].copy_from_slice(&root.0.to_le_bytes());
        image[record + 6..record + 10].copy_from_slice(&root.0.to_be_bytes());
        image[record + 10..record + 14].copy_from_slice(&root.1.to_le_bytes());
        image[record + 14..record + 18].copy_from_slice(&root.1.to_be_bytes());
        image[record + 25] = 0x02;
        image[record + 32] = 1;
        image[record + 33] = 0;
    }
}

/// A minimal UDF writer: an anchor, a four-descriptor volume sequence, a file set, and a
/// tree of File Entries and File Identifier Descriptors.
///
/// Shaped after the real `GRMSDK_EN_DVD.iso` rather than after the specification's full
/// generality: 2048-byte blocks, one partition, short allocation descriptors, and directories
/// embedded in their File Entry when they fit — which is exactly what that image does with
/// its root.
///
/// Descriptor tag checksums and CRCs are left zero. The reader does not check them, and a
/// fixture that computed them would be testing arithmetic nobody runs.
pub mod udf {
    use std::collections::BTreeMap;

    const BLOCK: usize = 2048;
    /// Where the partition starts, in blocks from the start of the image. Any value past the
    /// volume descriptor sequence works; this one leaves the sequence room to grow.
    const PARTITION_START: u32 = 264;

    /// Descriptor tag identifiers the reader looks for.
    const TAG_PRIMARY_VOLUME: u16 = 1;
    const TAG_ANCHOR: u16 = 2;
    const TAG_PARTITION: u16 = 5;
    const TAG_LOGICAL_VOLUME: u16 = 6;
    const TAG_TERMINATING: u16 = 8;
    const TAG_FILE_SET: u16 = 256;
    const TAG_FILE_IDENTIFIER: u16 = 257;
    const TAG_FILE_ENTRY: u16 = 261;

    const FILE_ENTRY_HEADER: usize = 176;
    const FILE_TYPE_DIRECTORY: u8 = 4;
    const FILE_TYPE_REGULAR: u8 = 5;

    /// A directory being assembled.
    #[derive(Default)]
    struct Dir {
        dirs: BTreeMap<String, Dir>,
        files: BTreeMap<String, Vec<u8>>,
    }

    /// Collects paths, then lays them out into a UDF image.
    #[derive(Default)]
    pub struct UdfBuilder {
        root: Dir,
    }

    impl UdfBuilder {
        pub fn new() -> Self {
            Self::default()
        }

        /// Add a file at a `/`-separated path, creating parent directories as needed.
        pub fn file(mut self, path: &str, content: Vec<u8>) -> Self {
            let mut components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
            let Some(name) = components.pop() else {
                return self;
            };
            let mut current = &mut self.root;
            for component in components {
                current = current.dirs.entry(component.to_string()).or_default();
            }
            current.files.insert(name.to_string(), content);
            self
        }

        pub fn build(self) -> Vec<u8> {
            // Partition block 0 is the file set descriptor, block 1 the root's File Entry.
            let mut writes: Vec<(u32, Vec<u8>)> = Vec::new();
            let mut next = 2u32;
            place(&self.root, 1, &mut writes, &mut next);
            writes.push((0, file_set_descriptor(1)));

            let total_blocks = PARTITION_START + next;
            let mut image = vec![0u8; total_blocks as usize * BLOCK];

            put(&mut image, 256, &anchor(257, 4 * BLOCK as u32));
            put(&mut image, 257, &volume_descriptor(TAG_PRIMARY_VOLUME));
            put(&mut image, 258, &logical_volume_descriptor(0));
            put(&mut image, 259, &partition_descriptor(next));
            put(&mut image, 260, &volume_descriptor(TAG_TERMINATING));
            for (block, bytes) in writes {
                put(&mut image, PARTITION_START + block, &bytes);
            }
            image
        }
    }

    /// Lay out one directory: its files, then its subdirectories, then its own File Entry at
    /// the block already reserved for it.
    fn place(dir: &Dir, fe_block: u32, writes: &mut Vec<(u32, Vec<u8>)>, next: &mut u32) {
        // The parent entry comes first and carries no name, as the format requires.
        let mut identifiers = file_identifier("", true, true, fe_block);

        for (name, content) in &dir.files {
            let blocks = content.len().div_ceil(BLOCK).max(1) as u32;
            let data_block = *next;
            *next += blocks;
            writes.push((data_block, content.clone()));

            let child = *next;
            *next += 1;
            writes.push((
                child,
                file_entry(
                    FILE_TYPE_REGULAR,
                    content.len() as u64,
                    Some((data_block, content.len() as u32)),
                    None,
                ),
            ));
            identifiers.extend(file_identifier(name, false, false, child));
        }

        for (name, child_dir) in &dir.dirs {
            let child = *next;
            *next += 1;
            place(child_dir, child, writes, next);
            identifiers.extend(file_identifier(name, true, false, child));
        }

        // Embed the directory in its File Entry when it fits, and spill it into an extent
        // when it does not — both shapes appear on the real image.
        if identifiers.len() <= BLOCK - FILE_ENTRY_HEADER {
            writes.push((
                fe_block,
                file_entry(
                    FILE_TYPE_DIRECTORY,
                    identifiers.len() as u64,
                    None,
                    Some(identifiers),
                ),
            ));
        } else {
            let blocks = identifiers.len().div_ceil(BLOCK) as u32;
            let data_block = *next;
            *next += blocks;
            let length = identifiers.len() as u32;
            writes.push((data_block, identifiers));
            writes.push((
                fe_block,
                file_entry(
                    FILE_TYPE_DIRECTORY,
                    u64::from(length),
                    Some((data_block, length)),
                    None,
                ),
            ));
        }
    }

    fn put(image: &mut [u8], block: u32, bytes: &[u8]) {
        let start = block as usize * BLOCK;
        image[start..start + bytes.len()].copy_from_slice(bytes);
    }

    fn tagged(tag: u16, length: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; length];
        bytes[0..2].copy_from_slice(&tag.to_le_bytes());
        // Descriptor version 2; checksum and CRC stay zero.
        bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
        bytes
    }

    fn anchor(sequence_block: u32, sequence_length: u32) -> Vec<u8> {
        let mut bytes = tagged(TAG_ANCHOR, BLOCK);
        bytes[16..20].copy_from_slice(&sequence_length.to_le_bytes());
        bytes[20..24].copy_from_slice(&sequence_block.to_le_bytes());
        bytes
    }

    fn volume_descriptor(tag: u16) -> Vec<u8> {
        tagged(tag, BLOCK)
    }

    fn logical_volume_descriptor(file_set_block: u32) -> Vec<u8> {
        let mut bytes = tagged(TAG_LOGICAL_VOLUME, BLOCK);
        bytes[212..216].copy_from_slice(&(BLOCK as u32).to_le_bytes());
        // `LogicalVolumeContentsUse` is a long_ad pointing at the file set descriptor.
        bytes[248..252].copy_from_slice(&(BLOCK as u32).to_le_bytes());
        bytes[252..256].copy_from_slice(&file_set_block.to_le_bytes());
        bytes
    }

    fn partition_descriptor(length_blocks: u32) -> Vec<u8> {
        let mut bytes = tagged(TAG_PARTITION, BLOCK);
        bytes[188..192].copy_from_slice(&PARTITION_START.to_le_bytes());
        bytes[192..196].copy_from_slice(&length_blocks.to_le_bytes());
        bytes
    }

    fn file_set_descriptor(root_block: u32) -> Vec<u8> {
        let mut bytes = tagged(TAG_FILE_SET, BLOCK);
        // `RootDirectoryICB`, a long_ad at offset 400.
        bytes[400..404].copy_from_slice(&(BLOCK as u32).to_le_bytes());
        bytes[404..408].copy_from_slice(&root_block.to_le_bytes());
        bytes
    }

    /// A File Entry with either one recorded extent or an embedded payload.
    fn file_entry(
        file_type: u8,
        information_length: u64,
        extent: Option<(u32, u32)>,
        embedded: Option<Vec<u8>>,
    ) -> Vec<u8> {
        let mut bytes = tagged(TAG_FILE_ENTRY, BLOCK);
        // ICB tag at 16: `FileType` 11 bytes in, `Flags` 18 bytes in.
        bytes[16 + 11] = file_type;
        let flags: u16 = if embedded.is_some() { 3 } else { 0 };
        bytes[16 + 18..16 + 20].copy_from_slice(&flags.to_le_bytes());
        bytes[56..64].copy_from_slice(&information_length.to_le_bytes());

        match embedded {
            Some(data) => {
                bytes[168..172].copy_from_slice(&0u32.to_le_bytes());
                bytes[172..176].copy_from_slice(&(data.len() as u32).to_le_bytes());
                bytes[176..176 + data.len()].copy_from_slice(&data);
            }
            None => {
                let (block, length) = extent.unwrap_or((0, 0));
                bytes[168..172].copy_from_slice(&0u32.to_le_bytes());
                bytes[172..176].copy_from_slice(&8u32.to_le_bytes());
                // A short_ad: recorded-and-allocated (extent type 0) plus the block number.
                bytes[176..180].copy_from_slice(&length.to_le_bytes());
                bytes[180..184].copy_from_slice(&block.to_le_bytes());
            }
        }
        bytes
    }

    /// One File Identifier Descriptor, padded to the four-byte boundary the format requires.
    fn file_identifier(name: &str, is_directory: bool, is_parent: bool, icb_block: u32) -> Vec<u8> {
        // A `d-string`: the compression byte says the characters are 8-bit.
        let identifier: Vec<u8> = if is_parent {
            Vec::new()
        } else {
            std::iter::once(8u8).chain(name.bytes()).collect()
        };
        let length = 38 + identifier.len();
        let padded = length.div_ceil(4) * 4;

        let mut bytes = tagged(TAG_FILE_IDENTIFIER, padded);
        bytes[16..18].copy_from_slice(&1u16.to_le_bytes());
        let mut characteristics = 0u8;
        if is_directory {
            characteristics |= 0x02;
        }
        if is_parent {
            characteristics |= 0x08;
        }
        bytes[18] = characteristics;
        bytes[19] = identifier.len() as u8;
        // The ICB, a long_ad at offset 20.
        bytes[20..24].copy_from_slice(&(BLOCK as u32).to_le_bytes());
        bytes[24..28].copy_from_slice(&icb_block.to_le_bytes());
        bytes[36..38].copy_from_slice(&0u16.to_le_bytes());
        bytes[38..38 + identifier.len()].copy_from_slice(&identifier);
        bytes
    }
}

/// A CAB written with the `cab` crate's own builder.
pub mod cabinet {
    use std::io::{Cursor, Write};

    /// Pack `files` — `(name inside the cabinet, contents)` — into an MSZIP cabinet.
    pub fn build(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = cab::CabinetBuilder::new();
        {
            let folder = builder.add_folder(cab::CompressionType::MsZip);
            for (name, _) in files {
                folder.add_file(*name);
            }
        }
        let mut writer = builder
            .build(Cursor::new(Vec::new()))
            .expect("the cab builder accepts this layout");
        let mut index = 0;
        while let Some(mut file) = writer.next_file().expect("cab writer advances") {
            file.write_all(files[index].1).expect("cab file write");
            index += 1;
        }
        writer.finish().expect("cab writer finishes").into_inner()
    }
}

/// An MSI written with the `msi` crate's own builder, carrying the four tables the extractor
/// reads: `Directory`, `Component`, `File` and `Media`.
pub mod package {
    use std::io::Cursor;

    use msi::{Column, Insert, Value};

    /// One file as an MSI describes it.
    pub struct FileRow {
        /// The `File` key — and, for a compressed package, the name inside the CAB.
        pub key: &'static str,
        /// `FileName`, either `short` or `short|long`.
        pub file_name: &'static str,
        /// The `Component` this file belongs to.
        pub component: &'static str,
        pub sequence: i32,
    }

    /// One `Directory` row: key, parent key (empty for the root), and `DefaultDir`.
    pub struct DirectoryRow {
        pub key: &'static str,
        pub parent: &'static str,
        pub default_dir: &'static str,
    }

    /// Build an MSI whose `Media` table is `(last sequence, cabinet name)` rows — several of
    /// them for a package split across cabinets, as `WinSDKBuild_x86.msi` is.
    pub fn build(
        directories: &[DirectoryRow],
        components: &[(&'static str, &'static str)],
        files: &[FileRow],
        media: &[(i32, &str)],
    ) -> Vec<u8> {
        let mut package =
            msi::Package::create(msi::PackageType::Installer, Cursor::new(Vec::new()))
                .expect("an empty installer package can be created");

        package
            .create_table(
                "Directory",
                vec![
                    Column::build("Directory").primary_key().id_string(72),
                    Column::build("Directory_Parent").nullable().id_string(72),
                    Column::build("DefaultDir")
                        .category(msi::Category::DefaultDir)
                        .string(255),
                ],
            )
            .expect("Directory table");
        package
            .create_table(
                "Component",
                vec![
                    Column::build("Component").primary_key().id_string(72),
                    Column::build("Directory_").id_string(72),
                ],
            )
            .expect("Component table");
        package
            .create_table(
                "File",
                vec![
                    Column::build("File").primary_key().id_string(72),
                    Column::build("Component_").id_string(72),
                    Column::build("FileName")
                        .category(msi::Category::Filename)
                        .string(255),
                    Column::build("Sequence").int16(),
                ],
            )
            .expect("File table");
        package
            .create_table(
                "Media",
                vec![
                    Column::build("DiskId").primary_key().int16(),
                    Column::build("LastSequence").int16(),
                    Column::build("Cabinet").nullable().string(255),
                ],
            )
            .expect("Media table");

        let directory_rows: Vec<Vec<Value>> = directories
            .iter()
            .map(|row| {
                vec![
                    Value::Str(row.key.to_string()),
                    if row.parent.is_empty() {
                        Value::Null
                    } else {
                        Value::Str(row.parent.to_string())
                    },
                    Value::Str(row.default_dir.to_string()),
                ]
            })
            .collect();
        package
            .insert_rows(Insert::into("Directory").rows(directory_rows))
            .expect("Directory rows");

        let component_rows: Vec<Vec<Value>> = components
            .iter()
            .map(|(key, directory)| {
                vec![
                    Value::Str(key.to_string()),
                    Value::Str(directory.to_string()),
                ]
            })
            .collect();
        package
            .insert_rows(Insert::into("Component").rows(component_rows))
            .expect("Component rows");

        let file_rows: Vec<Vec<Value>> = files
            .iter()
            .map(|row| {
                vec![
                    Value::Str(row.key.to_string()),
                    Value::Str(row.component.to_string()),
                    Value::Str(row.file_name.to_string()),
                    Value::Int(row.sequence),
                ]
            })
            .collect();
        package
            .insert_rows(Insert::into("File").rows(file_rows))
            .expect("File rows");

        let media_rows: Vec<Vec<Value>> = media
            .iter()
            .enumerate()
            .map(|(index, (last_sequence, cabinet))| {
                vec![
                    Value::Int(index as i32 + 1),
                    Value::Int(*last_sequence),
                    Value::Str((*cabinet).to_string()),
                ]
            })
            .collect();
        package
            .insert_rows(Insert::into("Media").rows(media_rows))
            .expect("Media rows");

        package
            .into_inner()
            .expect("the package flushes")
            .into_inner()
    }
}

/// A `.deb`-shaped `ar` archive around an xz-compressed tarball holding one member.
///
/// Enough of the format for the reader in `src/deb.rs`: the magic, three members in the order
/// dpkg writes them, 60-byte plain-text headers, and even-offset padding.
pub mod deb {
    /// Build a package whose data tarball contains `member` with `contents`.
    pub fn package(member: &str, contents: &[u8]) -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            let _ = builder.append_data(&mut header, format!("./{member}"), contents);
            let _ = builder.finish();
        }
        let mut xz = Vec::new();
        let _ = lzma_rs::xz_compress(&mut tar_bytes.as_slice(), &mut xz);

        let mut deb = Vec::from(b"!<arch>\n");
        for (name, body) in [
            ("debian-binary", b"2.0\n".to_vec()),
            ("control.tar.xz", Vec::new()),
            ("data.tar.xz", xz),
        ] {
            deb.extend_from_slice(format!("{name:<16}0           0     0     100644  ").as_bytes());
            deb.extend_from_slice(format!("{:<10}", body.len()).as_bytes());
            deb.extend_from_slice(b"`\n");
            deb.extend_from_slice(&body);
            if body.len() % 2 == 1 {
                deb.push(b'\n');
            }
        }
        deb
    }
}
