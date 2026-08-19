//! Reading exports and imports out of 32-bit Windows binaries.
//!
//! LuaJIT is ABI-compatible with Lua 5.1 by design, so a build of it renamed to
//! `lua51_Win32.dll` satisfies the game - *if* it really exports every one of the 80 symbols
//! the game's binaries import. "By design" is not a check, and a DLL short one symbol fails
//! at load time with nothing to point the player at. So the question is asked directly of the
//! bytes: what does this candidate export, what do those consumers need, and what is left
//! over.
//!
//! This is a reader, not a loader. It looks at exactly the two data directories it needs and
//! ignores everything else a PE file contains, which keeps it to a few hundred lines of
//! bounds-checked arithmetic. Every input is a file chosen by the player, so nothing here may
//! panic or read past the end of a buffer: malformed input is [`None`], never an abort.

/// The PE32 optional-header magic. PE32+ (`0x20b`) is 64-bit, which the game is not.
const MAGIC_PE32: u16 = 0x10b;
/// The PE32+ optional-header magic, recognised only so its data directories can be found.
const MAGIC_PE32_PLUS: u16 = 0x20b;

/// Where the data directories sit relative to the start of the optional header. PE32+ widens
/// five fields between the magic and the directories, hence the two constants.
const DIRECTORIES_PE32: usize = 96;
const DIRECTORIES_PE32_PLUS: usize = 112;

/// Data directory 0 is the export table, directory 1 the import table.
const DIRECTORY_EXPORTS: usize = 0;
const DIRECTORY_IMPORTS: usize = 1;

/// One entry of the section table, as far as address translation cares.
struct Section {
    virtual_address: u32,
    virtual_size: u32,
    raw_size: u32,
    raw_offset: u32,
}

/// A parsed-just-enough PE image: the bytes, the section table, and where the directories are.
struct Pe<'a> {
    bytes: &'a [u8],
    sections: Vec<Section>,
    directories: usize,
}

impl<'a> Pe<'a> {
    /// Walk the headers far enough to translate addresses and find the two directories.
    ///
    /// Returns [`None`] for anything that is not a PE image, including one whose headers point
    /// outside the file - a truncated download looks exactly like that.
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        let e_lfanew = read_u32(bytes, 0x3c)? as usize;
        // The COFF header is 20 bytes after the 4-byte signature, and the optional header
        // follows it; if that much is not present there is nothing to read.
        let number_of_sections = read_u16(bytes, e_lfanew.checked_add(6)?)? as usize;
        let size_of_optional = read_u16(bytes, e_lfanew.checked_add(20)?)? as usize;
        let optional = e_lfanew.checked_add(24)?;

        let directories = match read_u16(bytes, optional)? {
            MAGIC_PE32 => optional.checked_add(DIRECTORIES_PE32)?,
            MAGIC_PE32_PLUS => optional.checked_add(DIRECTORIES_PE32_PLUS)?,
            _ => return None,
        };

        let table = optional.checked_add(size_of_optional)?;
        let mut sections = Vec::with_capacity(number_of_sections.min(96));
        for index in 0..number_of_sections {
            let entry = table.checked_add(index.checked_mul(40)?)?;
            sections.push(Section {
                virtual_size: read_u32(bytes, entry.checked_add(8)?)?,
                virtual_address: read_u32(bytes, entry.checked_add(12)?)?,
                raw_size: read_u32(bytes, entry.checked_add(16)?)?,
                raw_offset: read_u32(bytes, entry.checked_add(20)?)?,
            });
        }

        Some(Pe {
            bytes,
            sections,
            directories,
        })
    }

    /// The `(address, size)` pair of one data directory, or [`None`] if the image has no such
    /// directory or the directory is empty.
    fn directory(&self, index: usize) -> Option<(u32, u32)> {
        let entry = self.directories.checked_add(index.checked_mul(8)?)?;
        let address = read_u32(self.bytes, entry)?;
        let size = read_u32(self.bytes, entry.checked_add(4)?)?;
        if address == 0 {
            return None;
        }
        Some((address, size))
    }

    /// Translate a relative virtual address into an offset into the file on disk.
    ///
    /// A section is usually padded on disk to a larger size than it declares in memory, but
    /// occasionally the reverse - uninitialised data lives past the raw bytes - so the wider
    /// of the two is what a lookup is allowed to land in.
    fn offset(&self, rva: u32) -> Option<usize> {
        self.sections.iter().find_map(|section| {
            let span = section.virtual_size.max(section.raw_size);
            let within = rva.checked_sub(section.virtual_address)?;
            if within >= span {
                return None;
            }
            usize::try_from(section.raw_offset.checked_add(within)?).ok()
        })
    }

    /// Read a u32 addressed by RVA rather than by file offset.
    fn u32_at_rva(&self, rva: u32) -> Option<u32> {
        read_u32(self.bytes, self.offset(rva)?)
    }

    /// The NUL-terminated ASCII string at an RVA.
    ///
    /// Symbol and library names in a PE file are plain ASCII, so anything that is not is a
    /// sign the address was wrong and the answer is [`None`] rather than a lossy guess.
    fn string_at_rva(&self, rva: u32) -> Option<String> {
        let start = self.offset(rva)?;
        let rest = self.bytes.get(start..)?;
        let end = rest.iter().position(|byte| *byte == 0)?;
        let name = rest.get(..end)?;
        if !name.is_ascii() {
            return None;
        }
        String::from_utf8(name.to_vec()).ok()
    }
}

/// Read a little-endian u16, or [`None`] if it would run off the end.
fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let field = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([*field.first()?, *field.get(1)?]))
}

/// Read a little-endian u32, or [`None`] if it would run off the end.
fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let field = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([
        *field.first()?,
        *field.get(1)?,
        *field.get(2)?,
        *field.get(3)?,
    ]))
}

/// Every function name a DLL exports, in the order the export table lists them.
///
/// Exports by ordinal only have no name and are therefore invisible here, which is correct:
/// the game imports every Lua symbol by name, so a name is the only thing that can satisfy it.
///
/// [`None`] means the bytes are not a PE image at all. A PE image with no export table at all
/// is a legitimate answer of "exports nothing", so that is an empty vector.
pub fn exported_names(dll: &[u8]) -> Option<Vec<String>> {
    let pe = Pe::parse(dll)?;
    let Some((directory, _)) = pe.directory(DIRECTORY_EXPORTS) else {
        return Some(Vec::new());
    };
    let Some(table) = pe.offset(directory) else {
        return Some(Vec::new());
    };

    let count = read_u32(dll, table.checked_add(24)?)? as usize;
    let names = read_u32(dll, table.checked_add(32)?)?;

    let mut exported = Vec::with_capacity(count.min(4096));
    for index in 0..count {
        let slot = names.checked_add(u32::try_from(index.checked_mul(4)?).ok()?)?;
        let Some(name) = pe.u32_at_rva(slot).and_then(|rva| pe.string_at_rva(rva)) else {
            // One unreadable entry says the table is not what it claims; the rest of it cannot
            // be trusted to be a complete answer either, so stop rather than under-report.
            break;
        };
        exported.push(name);
    }
    Some(exported)
}

/// Every symbol a binary imports from a DLL whose name mentions `lua`.
///
/// Matching on the name rather than on an exact `lua51_Win32.dll` is deliberate: the four
/// consumers do not all spell it identically, and any Lua DLL the game binds to is a DLL the
/// replacement engine has to stand in for.
///
/// Imports by ordinal are skipped - they have no name to compare against an export table, and
/// the game has none of them.
pub fn imported_lua_names(binary: &[u8]) -> Option<Vec<String>> {
    let pe = Pe::parse(binary)?;
    let Some((directory, _)) = pe.directory(DIRECTORY_IMPORTS) else {
        return Some(Vec::new());
    };
    let Some(table) = pe.offset(directory) else {
        return Some(Vec::new());
    };

    let mut imported = Vec::new();
    // The descriptor array is terminated by an all-zero entry rather than by a count, so the
    // cap is what stops a corrupt file that never terminates from spinning here.
    const MAX_DESCRIPTORS: usize = 4096;
    for index in 0..MAX_DESCRIPTORS {
        let Some(entry) = index.checked_mul(20).and_then(|at| table.checked_add(at)) else {
            break;
        };
        let Some(original_first_thunk) = read_u32(binary, entry) else {
            break;
        };
        let Some(library) = read_u32(binary, entry.checked_add(12)?) else {
            break;
        };
        let Some(first_thunk) = read_u32(binary, entry.checked_add(16)?) else {
            break;
        };
        if original_first_thunk == 0 && library == 0 && first_thunk == 0 {
            break;
        }

        let Some(library) = pe.string_at_rva(library) else {
            continue;
        };
        if !library.to_ascii_lowercase().contains("lua") {
            continue;
        }

        // A bound import has its original thunk array stripped, leaving only `FirstThunk`,
        // which before loading still holds the same hint/name addresses.
        let thunks = if original_first_thunk == 0 {
            first_thunk
        } else {
            original_first_thunk
        };
        collect_thunk_names(&pe, thunks, &mut imported);
    }
    Some(imported)
}

/// Walk one NUL-terminated thunk array, pushing the name of every by-name import.
fn collect_thunk_names(pe: &Pe<'_>, thunks: u32, into: &mut Vec<String>) {
    /// As with the descriptors, the array ends with a zero rather than a count.
    const MAX_THUNKS: usize = 65536;
    for index in 0..MAX_THUNKS {
        let Some(slot) = u32::try_from(index * 4)
            .ok()
            .and_then(|at| thunks.checked_add(at))
        else {
            return;
        };
        let Some(thunk) = pe.u32_at_rva(slot) else {
            return;
        };
        if thunk == 0 {
            return;
        }
        // The high bit marks an import by ordinal, which carries no name at all.
        if thunk & 0x8000_0000 != 0 {
            continue;
        }
        // The name follows a two-byte hint the loader is free to ignore, and so do we.
        let Some(name) = thunk.checked_add(2).and_then(|at| pe.string_at_rva(at)) else {
            continue;
        };
        into.push(name);
    }
}

/// What `consumers` need from a Lua DLL that `dll` does not provide, sorted and deduplicated.
///
/// An empty vector is the answer that clears a candidate engine for deployment. [`None`] means
/// one of the files handed in did not parse as a PE image, which is a different thing
/// entirely: not "this DLL is unsuitable" but "the question could not be asked", and the
/// caller has to treat it as such rather than as a pass.
pub fn missing_for(dll: &[u8], consumers: &[&[u8]]) -> Option<Vec<String>> {
    let exported = exported_names(dll)?;
    let mut missing = Vec::new();
    for consumer in consumers {
        for needed in imported_lua_names(consumer)? {
            if !exported.contains(&needed) {
                missing.push(needed);
            }
        }
    }
    missing.sort();
    missing.dedup();
    Some(missing)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable
)]
mod tests {
    use super::*;

    /// Where the one section of a fixture image lives, in memory and on disk. The values are
    /// arbitrary; they only have to leave room for the headers ahead of them.
    const SECTION_RVA: u32 = 0x1000;
    const SECTION_RAW: u32 = 0x400;

    /// Build a PE32 image around one section, with one data directory pointing into it.
    ///
    /// This is the smallest thing the parser accepts: a DOS stub whose `e_lfanew` reaches a
    /// COFF header, an optional header long enough to hold the data directories, and a single
    /// section table entry to translate addresses through. Hand-assembling it keeps the tests
    /// free of a linker and lets them describe malformed images just as easily as valid ones.
    fn pe_image(section: &[u8], directory: usize, rva: u32, size: u32) -> Vec<u8> {
        const E_LFANEW: usize = 0x80;
        const SIZE_OF_OPTIONAL: usize = 224;
        let optional = E_LFANEW + 24;

        let mut image = vec![0u8; SECTION_RAW as usize];
        image[0..2].copy_from_slice(b"MZ");
        image[0x3c..0x40].copy_from_slice(&(E_LFANEW as u32).to_le_bytes());

        image[E_LFANEW..E_LFANEW + 4].copy_from_slice(b"PE\0\0");
        // Machine 0x14c is i386, which is what everything in this crate targets.
        image[E_LFANEW + 4..E_LFANEW + 6].copy_from_slice(&0x14cu16.to_le_bytes());
        image[E_LFANEW + 6..E_LFANEW + 8].copy_from_slice(&1u16.to_le_bytes());
        image[E_LFANEW + 20..E_LFANEW + 22]
            .copy_from_slice(&(SIZE_OF_OPTIONAL as u16).to_le_bytes());

        image[optional..optional + 2].copy_from_slice(&MAGIC_PE32.to_le_bytes());
        let entry = optional + DIRECTORIES_PE32 + directory * 8;
        image[entry..entry + 4].copy_from_slice(&rva.to_le_bytes());
        image[entry + 4..entry + 8].copy_from_slice(&size.to_le_bytes());

        let table = optional + SIZE_OF_OPTIONAL;
        let length = u32::try_from(section.len()).unwrap();
        image[table + 8..table + 12].copy_from_slice(&length.to_le_bytes());
        image[table + 12..table + 16].copy_from_slice(&SECTION_RVA.to_le_bytes());
        image[table + 16..table + 20].copy_from_slice(&length.to_le_bytes());
        image[table + 20..table + 24].copy_from_slice(&SECTION_RAW.to_le_bytes());

        image.extend_from_slice(section);
        image
    }

    /// The RVA of an offset within the fixture's single section.
    fn rva(offset: usize) -> u32 {
        SECTION_RVA + u32::try_from(offset).unwrap()
    }

    /// A DLL exporting exactly `names`, by name.
    fn exports_fixture(names: &[&str]) -> Vec<u8> {
        // Export directory, then the array of name pointers, then the names themselves.
        let directory = 40usize;
        let pointers = directory + 40;
        let mut section = vec![0u8; pointers + names.len() * 4];

        section[directory + 24..directory + 28]
            .copy_from_slice(&u32::try_from(names.len()).unwrap().to_le_bytes());
        section[directory + 32..directory + 36].copy_from_slice(&rva(pointers).to_le_bytes());

        for (index, name) in names.iter().enumerate() {
            let at = section.len();
            section.extend_from_slice(name.as_bytes());
            section.push(0);
            let slot = pointers + index * 4;
            section[slot..slot + 4].copy_from_slice(&rva(at).to_le_bytes());
        }

        pe_image(&section, DIRECTORY_EXPORTS, rva(directory), 0)
    }

    /// A binary importing exactly `names` from a Lua DLL, plus one symbol from a DLL that has
    /// nothing to do with Lua so the name filter is actually exercised.
    fn imports_fixture(names: &[&str]) -> Vec<u8> {
        let descriptors = 0usize;
        let thunks = 60usize;
        let other_thunks = thunks + (names.len() + 1) * 4;
        let mut section = vec![0u8; other_thunks + 8];

        let lua_name = section.len();
        section.extend_from_slice(b"lua51_Win32.dll\0");
        let other_name = section.len();
        section.extend_from_slice(b"KERNEL32.dll\0");

        let hint_name = |section: &mut Vec<u8>, name: &str| {
            let at = section.len();
            section.extend_from_slice(&0u16.to_le_bytes());
            section.extend_from_slice(name.as_bytes());
            section.push(0);
            rva(at)
        };

        for (index, name) in names.iter().enumerate() {
            let pointer = hint_name(&mut section, name);
            let slot = thunks + index * 4;
            section[slot..slot + 4].copy_from_slice(&pointer.to_le_bytes());
        }
        let pointer = hint_name(&mut section, "Sleep");
        section[other_thunks..other_thunks + 4].copy_from_slice(&pointer.to_le_bytes());

        // Descriptor one: the Lua DLL. Descriptor two: the unrelated one. Descriptor three is
        // the all-zero terminator the section is already padded with.
        section[descriptors..descriptors + 4].copy_from_slice(&rva(thunks).to_le_bytes());
        section[descriptors + 12..descriptors + 16].copy_from_slice(&rva(lua_name).to_le_bytes());
        section[descriptors + 16..descriptors + 20].copy_from_slice(&rva(thunks).to_le_bytes());
        section[descriptors + 20..descriptors + 24]
            .copy_from_slice(&rva(other_thunks).to_le_bytes());
        section[descriptors + 32..descriptors + 36].copy_from_slice(&rva(other_name).to_le_bytes());
        section[descriptors + 36..descriptors + 40]
            .copy_from_slice(&rva(other_thunks).to_le_bytes());

        pe_image(&section, DIRECTORY_IMPORTS, rva(descriptors), 0)
    }

    /// The real game, if the machine running the tests has one.
    fn game_directory_from_env() -> Option<std::path::PathBuf> {
        std::env::var_os("CIV5_GAME_DIR").map(std::path::PathBuf::from)
    }

    /// The stock engine trivially satisfies the game - which is what proves the checker
    /// itself is right before it is trusted to judge our own build.
    #[test]
    #[ignore = "needs a real Civilization V installation"]
    fn the_stock_engine_satisfies_the_game() {
        let Some(game) = game_directory_from_env() else {
            return;
        };
        let dll = std::fs::read(game.join("lua51_Win32.dll")).expect("the engine");
        let exe = std::fs::read(game.join("CivilizationV_DX11.exe")).expect("the game");
        // Without this the test would pass on a parser that found nothing at all, which is the
        // one way a checker like this fails silently.
        let Some(needed) = imported_lua_names(&exe) else {
            unreachable!("the game parses as PE")
        };
        assert!(!needed.is_empty(), "the game imports Lua symbols");

        let Some(missing) = missing_for(&dll, &[&exe]) else {
            unreachable!("both files parse as PE")
        };
        assert!(missing.is_empty(), "stock must satisfy stock: {missing:?}");
    }

    /// A DLL that exports nothing must be reported as missing everything - the checker has
    /// to actually fail, or it would pass a broken build too.
    #[test]
    fn a_dll_exporting_nothing_is_reported_as_missing_everything() {
        let empty = exports_fixture(&[]);
        let consumer = imports_fixture(&["lua_pcall", "lua_gettop"]);
        let Some(missing) = missing_for(&empty, &[&consumer]) else {
            unreachable!("the fixtures parse as PE")
        };
        assert_eq!(
            missing,
            vec!["lua_gettop".to_owned(), "lua_pcall".to_owned()]
        );
    }

    /// The pass case, and the proof that the filter keeps non-Lua imports out of the demand:
    /// the fixture also imports `Sleep` from `KERNEL32.dll`, which no Lua engine exports.
    #[test]
    fn a_dll_exporting_everything_is_missing_nothing() {
        let full = exports_fixture(&["lua_pcall", "lua_gettop"]);
        let consumer = imports_fixture(&["lua_pcall", "lua_gettop"]);
        assert_eq!(
            exported_names(&full),
            Some(vec!["lua_pcall".to_owned(), "lua_gettop".to_owned()])
        );
        assert_eq!(
            imported_lua_names(&consumer),
            Some(vec!["lua_pcall".to_owned(), "lua_gettop".to_owned()])
        );
        assert_eq!(missing_for(&full, &[&consumer]), Some(Vec::new()));
    }

    /// Two consumers wanting the same absent symbol name it once, because the report goes in
    /// front of a person.
    #[test]
    fn the_same_missing_symbol_is_reported_once() {
        let empty = exports_fixture(&[]);
        let one = imports_fixture(&["lua_pcall"]);
        let two = imports_fixture(&["lua_pcall"]);
        assert_eq!(
            missing_for(&empty, &[&one, &two]),
            Some(vec!["lua_pcall".to_owned()])
        );
    }

    /// A file that stops partway through its own headers must be refused, not read past.
    #[test]
    fn a_truncated_image_does_not_parse() {
        let full = exports_fixture(&["lua_pcall"]);
        for length in [0, 1, 0x3c, 0x40, 0x90, 0x120, 0x170] {
            assert_eq!(
                exported_names(&full[..length]),
                None,
                "a {length}-byte prefix is not a PE image"
            );
        }
    }

    /// `e_lfanew` is attacker-controlled in the sense that matters here: it is a raw offset out
    /// of a file the player chose, and a huge one must be a refusal rather than a read.
    #[test]
    fn an_e_lfanew_past_the_end_does_not_parse() {
        let mut image = exports_fixture(&["lua_pcall"]);
        image[0x3c..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(exported_names(&image), None);
        assert_eq!(imported_lua_names(&image), None);
        assert_eq!(missing_for(&image, &[]), None);
    }

    /// Whatever the bytes are, the answer is a value and not a crash.
    #[test]
    fn arbitrary_bytes_are_refused_rather_than_trusted() {
        let mut state = 0x1234_5678u32;
        for length in [1usize, 7, 64, 300, 2048] {
            let garbage: Vec<u8> = (0..length)
                .map(|_| {
                    // A tiny xorshift, so the "fuzzing" is reproducible and needs no crate.
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    (state & 0xff) as u8
                })
                .collect();
            let _ = exported_names(&garbage);
            let _ = imported_lua_names(&garbage);
            let _ = missing_for(&garbage, &[&garbage]);
        }
    }
}
