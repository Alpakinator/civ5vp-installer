//! A minimal PE32 reader for the real-build test: machine, DLL bit, export names, imported
//! DLL names. Test-only - the installer itself never parses PE files - so this reads just
//! enough of the format to compare a built DLL with the reference, and errors loudly rather
//! than guessing.

use std::collections::BTreeSet;

/// `IMAGE_FILE_MACHINE_I386`.
pub const MACHINE_I386: u16 = 0x014C;

/// What the comparison needs from one PE file.
#[derive(Debug)]
pub struct PortableExecutable {
    pub machine: u16,
    pub is_dll: bool,
    pub exports: BTreeSet<String>,
    pub imported_dlls: BTreeSet<String>,
}

struct Section {
    virtual_address: u32,
    virtual_size: u32,
    raw_offset: u32,
}

pub fn parse(bytes: &[u8]) -> Result<PortableExecutable, String> {
    if bytes.get(..2) != Some(b"MZ") {
        return Err("no MZ header".into());
    }
    let pe_offset = u32_at(bytes, 0x3C)? as usize;
    if bytes.get(pe_offset..pe_offset + 4) != Some(b"PE\0\0") {
        return Err("no PE signature".into());
    }
    let coff = pe_offset + 4;
    let machine = u16_at(bytes, coff)?;
    let section_count = u16_at(bytes, coff + 2)? as usize;
    let optional_size = u16_at(bytes, coff + 16)? as usize;
    let characteristics = u16_at(bytes, coff + 18)?;
    let is_dll = characteristics & 0x2000 != 0;

    let optional = coff + 20;
    let magic = u16_at(bytes, optional)?;
    if magic != 0x10B {
        return Err(format!("not PE32 (optional header magic {magic:#06x})"));
    }
    let directory_count = u32_at(bytes, optional + 92)? as usize;
    let directories = optional + 96;

    let mut sections = Vec::with_capacity(section_count);
    let section_table = optional + optional_size;
    for index in 0..section_count {
        let entry = section_table + index * 40;
        sections.push(Section {
            virtual_size: u32_at(bytes, entry + 8)?,
            virtual_address: u32_at(bytes, entry + 12)?,
            raw_offset: u32_at(bytes, entry + 20)?,
        });
    }

    let directory = |index: usize| -> Result<Option<(u32, u32)>, String> {
        if index >= directory_count {
            return Ok(None);
        }
        let entry = directories + index * 8;
        let rva = u32_at(bytes, entry)?;
        let size = u32_at(bytes, entry + 4)?;
        Ok((rva != 0).then_some((rva, size)))
    };

    let mut exports = BTreeSet::new();
    if let Some((rva, _)) = directory(0)? {
        let table = rva_to_offset(&sections, rva).ok_or("export table RVA maps nowhere")?;
        let name_count = u32_at(bytes, table + 24)? as usize;
        let names_rva = u32_at(bytes, table + 32)?;
        let names = rva_to_offset(&sections, names_rva).ok_or("export names RVA maps nowhere")?;
        for index in 0..name_count {
            let name_rva = u32_at(bytes, names + index * 4)?;
            let name = rva_to_offset(&sections, name_rva).ok_or("export name maps nowhere")?;
            exports.insert(c_string(bytes, name)?);
        }
    }

    let mut imported_dlls = BTreeSet::new();
    if let Some((rva, _)) = directory(1)? {
        let mut descriptor = rva_to_offset(&sections, rva).ok_or("import table maps nowhere")?;
        loop {
            let name_rva = u32_at(bytes, descriptor + 12)?;
            if name_rva == 0 {
                break;
            }
            let name = rva_to_offset(&sections, name_rva).ok_or("import name maps nowhere")?;
            imported_dlls.insert(c_string(bytes, name)?);
            descriptor += 20;
        }
    }

    Ok(PortableExecutable {
        machine,
        is_dll,
        exports,
        imported_dlls,
    })
}

fn rva_to_offset(sections: &[Section], rva: u32) -> Option<usize> {
    sections
        .iter()
        .find(|section| {
            rva >= section.virtual_address && rva < section.virtual_address + section.virtual_size
        })
        .map(|section| (rva - section.virtual_address + section.raw_offset) as usize)
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, String> {
    bytes
        .get(offset..offset + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| format!("truncated at {offset:#x}"))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, String> {
    bytes
        .get(offset..offset + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("truncated at {offset:#x}"))
}

fn c_string(bytes: &[u8], offset: usize) -> Result<String, String> {
    let tail = bytes
        .get(offset..)
        .ok_or_else(|| format!("string offset {offset:#x} out of range"))?;
    let end = tail
        .iter()
        .position(|&b| b == 0)
        .ok_or("unterminated string")?;
    Ok(String::from_utf8_lossy(&tail[..end]).into_owned())
}
