//! The game's own interface art, read from the player's installed copy of Civilization V.
//!
//! The skin is the Vox Populi Civilopedia's (visual language, "Game art"): the base game's
//! main-menu painting behind the page, and the Wonder popup's corner fans in each panel's top
//! corners. None of it is in this binary. It is read at runtime from the game folder the
//! player already has, the way the Civilopedia reads it at build time, so nothing of
//! Firaxis's is redistributed (ADR-0012). Without a readable game, the skin goes without it:
//! the page is plain navy and the panels have no fans.
//!
//! Read only. Nothing here writes anywhere, so it stands outside the Claimed Folders rule
//! entirely (rule 6) - and it never fails loudly: art that is missing or unreadable is a
//! cosmetic loss, logged and dropped.
//!
//! The format, as `tools/civ5_art.py` in the Civilopedia reads it: an FPK v6 archive is a
//! bounded index followed by uncompressed members. An interface texture is a DDS whose pixels
//! are packed in square blocks, each block placed in the picture by a companion `-index.dds`
//! texture; decoding it straight as a DDS scrambles it.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// The pictures this skin borrows from the game, decoded to RGBA.
pub struct GameArt {
    /// The base game's main-menu painting, a statue under a vaulted sky.
    pub backdrop: Option<egui::ColorImage>,
    /// The Wonder popup's top-left and top-right corners, lit to the rim they lie under.
    pub fans: Option<[egui::ColorImage; 2]>,
}

impl GameArt {
    /// Whatever of the art `game_root` holds. Each piece is read on its own, so a game
    /// missing one archive still gives the others.
    pub fn read(game_root: &Path) -> Self {
        let dx9 = game_root.join("Resource").join("DX9");
        // The base game's own menu painting: the expansions carry their own paintings under
        // the same name, and the Civilopedia chose this one (`splash_art.py`, `menu.webp`).
        let backdrop = picture(&dx9.join("UITextures.fpk"), "CivilzationVAtlas.dds");
        // The fans from the latest layer that has them: Brave New World's, which Vox Populi
        // requires, over the base game's.
        let fans = ["Expansion2UITextures.fpk", "UITextures.fpk"]
            .iter()
            .find_map(|archive| {
                let archive = dx9.join(archive);
                let left = picture(&archive, "TopLeftCorner.dds").ok()?;
                let right = picture(&archive, "TopRightCorner.dds").ok()?;
                Some([light_fan(left, false), light_fan(right, true)])
            });
        if let Err(problem) = &backdrop {
            crate::log_detail(&format!("Game art: no menu painting ({problem})."));
        }
        if fans.is_none() {
            crate::log_detail("Game art: no corner fans in the game's interface archives.");
        }
        Self {
            backdrop: backdrop.ok(),
            fans,
        }
    }
}

/// One interface texture, decoded.
fn picture(archive: &Path, name: &str) -> Result<egui::ColorImage, String> {
    let index_name = format!("{}-index.dds", name.trim_end_matches(".dds"));
    let mut archive = Archive::open(archive)?;
    let texture = archive.read(name)?;
    let index = archive.read(&index_name)?;
    decode(&texture, &index)
}

// ── FPK ──────────────────────────────────────────────────────────────────────

struct Archive {
    file: BufReader<File>,
    /// Each member's case-folded name, offset and size.
    members: Vec<(String, u64, u64)>,
}

/// More members than any of the game's archives has; past it the index is not one.
const MAX_MEMBERS: u32 = 100_000;
/// The largest member read: the biggest interface texture is ~9 MB.
const MAX_MEMBER: u64 = 64 << 20;

impl Archive {
    fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
        let length = file.metadata().map_err(|err| err.to_string())?.len();
        let mut file = BufReader::new(file);
        let mut magic = [0; 10];
        file.read_exact(&mut magic).map_err(|err| err.to_string())?;
        if &magic != b"\x06\x00\x00\x00FPK_\x00\x00" {
            return Err(format!("{} is not an FPK v6 archive", path.display()));
        }
        let count = read_u32(&mut file)?;
        if count > MAX_MEMBERS {
            return Err("implausible member count".to_owned());
        }
        let mut members = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let name_length = read_u32(&mut file)? as usize;
            if !(1..4096).contains(&name_length) {
                return Err("implausible member name".to_owned());
            }
            let mut name = vec![0; name_length.div_ceil(4) * 4];
            file.read_exact(&mut name).map_err(|err| err.to_string())?;
            name.truncate(name_length);
            while name.last() == Some(&0) {
                name.pop();
            }
            let name = String::from_utf8_lossy(&name).to_lowercase();
            let _ = (read_u32(&mut file)?, read_u32(&mut file)?);
            let size = u64::from(read_u32(&mut file)?);
            let offset = u64::from(read_u32(&mut file)?);
            if offset + size > length {
                return Err("member runs past the archive".to_owned());
            }
            members.push((name, offset, size));
        }
        Ok(Self { file, members })
    }

    fn read(&mut self, name: &str) -> Result<Vec<u8>, String> {
        let wanted = name.to_lowercase();
        let &(_, offset, size) = self
            .members
            .iter()
            .find(|(member, _, _)| *member == wanted)
            .ok_or_else(|| format!("{name} is not in the archive"))?;
        if size > MAX_MEMBER {
            return Err(format!("{name} is implausibly large"));
        }
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|err| err.to_string())?;
        let mut data = vec![0; size as usize];
        self.file
            .read_exact(&mut data)
            .map_err(|err| err.to_string())?;
        Ok(data)
    }
}

fn read_u32(reader: &mut impl Read) -> Result<u32, String> {
    let mut bytes = [0; 4];
    reader
        .read_exact(&mut bytes)
        .map_err(|err| err.to_string())?;
    Ok(u32::from_le_bytes(bytes))
}

// ── Block-indexed DDS ────────────────────────────────────────────────────────

fn u32_at(data: &[u8], at: usize) -> Result<u32, String> {
    data.get(at..at + 4)
        .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .ok_or_else(|| "truncated DDS header".to_owned())
}

/// A DDS header's width and height.
fn dimensions(data: &[u8]) -> Result<(usize, usize), String> {
    if data.len() < 128 || &data[..4] != b"DDS " || u32_at(data, 4)? != 124 {
        return Err("not a DDS texture".to_owned());
    }
    let (height, width) = (u32_at(data, 12)? as usize, u32_at(data, 16)? as usize);
    if width == 0 || height == 0 || width * height > 1 << 26 {
        return Err("implausible DDS dimensions".to_owned());
    }
    Ok((width, height))
}

/// The pixel rows of a DDS, `bytes` per pixel, with any declared pitch dropped. The game's
/// index textures have odd row lengths and declare no pitch: they are not DWORD-aligned.
fn rows(data: &[u8], width: usize, height: usize, bytes: usize) -> Result<Vec<u8>, String> {
    let row = width * bytes;
    let declared = u32_at(data, 20)? as usize;
    let pitch = if u32_at(data, 8)? & 8 != 0 {
        declared
    } else {
        row
    };
    if pitch < row || data.len() - 128 < pitch * height {
        return Err("truncated DDS rows".to_owned());
    }
    Ok((0..height)
        .flat_map(|y| &data[128 + y * pitch..128 + y * pitch + row])
        .copied()
        .collect())
}

/// An interface texture put back together: each square block of `texture` placed where
/// `index` says. Only the 32-bit BGRA kind the art here uses is read.
fn decode(texture: &[u8], index: &[u8]) -> Result<egui::ColorImage, String> {
    let (width, height) = dimensions(texture)?;
    let marker = &texture[116..121];
    if &marker[..2] != b"BC" {
        return Err("not a block-indexed interface texture".to_owned());
    }
    let block = std::str::from_utf8(&marker[2..])
        .ok()
        .and_then(|digits| usize::from_str_radix(digits, 16).ok())
        .filter(|block| block.is_power_of_two() && *block <= 512)
        .ok_or_else(|| "unreadable block size".to_owned())?;
    let format = [88, 92, 96, 100, 104].map(|at| u32_at(texture, at));
    if u32_at(texture, 84)? != 0
        || format.iter().any(Result::is_err)
        || format.map(Result::unwrap_or_default) != [32, 0xFF_0000, 0xFF00, 0xFF, 0xFF00_0000]
    {
        return Err("not a 32-bit BGRA texture".to_owned());
    }
    if width % block != 0 || height % block != 0 {
        return Err("texture is not whole blocks".to_owned());
    }
    let pixels = rows(texture, width, height, 4)?;

    let (index_width, index_height) = dimensions(index)?;
    let index_bytes = match u32_at(index, 88)? {
        8 => 1,
        16 => 2,
        _ => return Err("unreadable index depth".to_owned()),
    };
    let indices: Vec<usize> = rows(index, index_width, index_height, index_bytes)?
        .chunks_exact(index_bytes)
        .map(|entry| {
            if index_bytes == 1 {
                usize::from(entry[0])
            } else {
                usize::from(u16::from_le_bytes([entry[0], entry[1]]))
            }
        })
        .collect();
    let per_row = width / block;
    if indices.iter().any(|&n| n >= per_row * (height / block)) {
        return Err("index points past the texture".to_owned());
    }

    let (out_width, out_height) = (index_width * block, index_height * block);
    if out_width * out_height > 1 << 26 {
        return Err("implausible picture size".to_owned());
    }
    let mut rgba = Vec::with_capacity(out_width * out_height * 4);
    for y in 0..out_height {
        let (block_y, within) = (y / block, y % block);
        for x in 0..index_width {
            let n = indices[block_y * index_width + x];
            let start = ((n / per_row * block + within) * width + n % per_row * block) * 4;
            for bgra in pixels[start..start + block * 4].chunks_exact(4) {
                rgba.extend_from_slice(&[bgra[2], bgra[1], bgra[0], bgra[3]]);
            }
        }
    }
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [out_width, out_height],
        &rgba,
    ))
}

// ── Lighting the fans ────────────────────────────────────────────────────────

/// A fan lit as the rim it lies under (the Civilopedia's `light_fan`): each row brightened
/// or darkened by a factor that passes, as the rim does, from the rim's top tone to its side
/// tone over the first [`crate::deco::RIM_PASS`] points, so the fan's gold just inside the
/// rim meets it at the rim's own brightness.
fn light_fan(mut fan: egui::ColorImage, mirrored: bool) -> egui::ColorImage {
    let [width, height] = fan.size;
    let scale = width as f32 / crate::deco::FAN;
    let depth = (crate::deco::RIM * scale).round() as usize;
    if depth >= width.min(height) {
        return fan;
    }
    let at = |fan: &egui::ColorImage, x: usize, y: usize| {
        let x = if mirrored { width - 1 - x } else { x };
        fan.pixels[y * width + x].to_srgba_unmultiplied()
    };
    let mean = |colours: Vec<[u8; 4]>| {
        let opaque: Vec<[u8; 4]> = colours.into_iter().filter(|c| c[3] > 200).collect();
        let count = opaque.len().max(1) as f32;
        luminance([0, 1, 2].map(|k| opaque.iter().map(|c| f32::from(c[k])).sum::<f32>() / count))
    };
    let top = mean((0..width).map(|x| at(&fan, x, depth)).collect());
    let side = mean((0..height).map(|y| at(&fan, depth, y)).collect());
    if top <= 0.0 || side <= 0.0 {
        return fan;
    }
    let tone = |c: egui::Color32| luminance([c.r(), c.g(), c.b()].map(f32::from));
    let (top_factor, side_factor) = (
        tone(crate::theme::RIM_TOP) / top,
        tone(crate::theme::RIM_SIDE) / side,
    );
    let passing = crate::deco::RIM_PASS * scale;
    for y in 0..height {
        let factor = top_factor + (side_factor - top_factor) * (y as f32 / passing).min(1.0);
        for pixel in &mut fan.pixels[y * width..(y + 1) * width] {
            let [r, g, b, a] = pixel.to_srgba_unmultiplied();
            let lit = |c: u8| (f32::from(c) * factor).round().min(255.0) as u8;
            *pixel = egui::Color32::from_rgba_unmultiplied(lit(r), lit(g), lit(b), a);
        }
    }
    fan
}

fn luminance([r, g, b]: [f32; 3]) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A DDS header with the fields the decoder reads.
    fn dds(width: u32, height: u32, bits: u32, masks: [u32; 4], marker: &[u8]) -> Vec<u8> {
        let mut header = vec![0; 128];
        header[..4].copy_from_slice(b"DDS ");
        header[4..8].copy_from_slice(&124u32.to_le_bytes());
        header[12..16].copy_from_slice(&height.to_le_bytes());
        header[16..20].copy_from_slice(&width.to_le_bytes());
        header[32..45].copy_from_slice(b"FTXTInterface");
        header[76..80].copy_from_slice(&32u32.to_le_bytes());
        header[88..92].copy_from_slice(&bits.to_le_bytes());
        for (k, mask) in masks.iter().enumerate() {
            header[92 + 4 * k..96 + 4 * k].copy_from_slice(&mask.to_le_bytes());
        }
        header[116..116 + marker.len()].copy_from_slice(marker);
        header
    }

    /// Two 2x2 blocks packed side by side, red then blue, and an index that lays them out
    /// blue over red: the picture comes back in the index's order, not the packing's.
    #[test]
    fn blocks_are_laid_out_where_the_index_says() {
        let mut texture = dds(4, 2, 32, [0xFF_0000, 0xFF00, 0xFF, 0xFF00_0000], b"BC002");
        // BGRA rows: each row is two red pixels then two blue ones.
        for _ in 0..2 {
            texture.extend([0, 0, 255, 255, 0, 0, 255, 255]);
            texture.extend([255, 0, 0, 255, 255, 0, 0, 255]);
        }
        let mut index = dds(1, 2, 16, [0; 4], b"BC002");
        index.extend(1u16.to_le_bytes());
        index.extend(0u16.to_le_bytes());

        let Ok(picture) = decode(&texture, &index) else {
            unreachable!("a well-formed texture decodes")
        };
        assert_eq!(picture.size, [2, 4]);
        assert_eq!(picture.pixels[0], egui::Color32::BLUE);
        assert_eq!(picture.pixels[3], egui::Color32::BLUE);
        assert_eq!(picture.pixels[4], egui::Color32::RED);
        assert_eq!(picture.pixels[7], egui::Color32::RED);
    }

    #[test]
    fn an_index_past_the_texture_is_refused() {
        let mut texture = dds(2, 2, 32, [0xFF_0000, 0xFF00, 0xFF, 0xFF00_0000], b"BC002");
        texture.extend([0; 16]);
        let mut index = dds(1, 1, 16, [0; 4], b"BC002");
        index.extend(1u16.to_le_bytes());
        assert!(decode(&texture, &index).is_err());
    }

    /// An archive with two members is read back by name, whatever the case.
    #[test]
    fn members_are_found_by_name() {
        let directory = tempfile::tempdir().unwrap_or_else(|err| unreachable!("{err}"));
        let path = directory.path().join("art.fpk");
        let mut bytes = b"\x06\x00\x00\x00FPK_\x00\x00".to_vec();
        bytes.extend(2u32.to_le_bytes());
        let header_end = 10 + 4 + 2 * (4 + 8 + 16);
        for (k, name) in [b"One.dds".as_slice(), b"Two.dds".as_slice()]
            .iter()
            .enumerate()
        {
            bytes.extend((name.len() as u32).to_le_bytes());
            bytes.extend(*name);
            bytes.push(0);
            bytes.extend([0; 8]);
            bytes.extend(3u32.to_le_bytes());
            bytes.extend((header_end as u32 + 3 * k as u32).to_le_bytes());
        }
        bytes.extend(b"abcxyz");
        std::fs::write(&path, bytes).unwrap_or_else(|err| unreachable!("{err}"));

        let Ok(mut archive) = Archive::open(&path) else {
            unreachable!("the archive opens")
        };
        assert_eq!(
            archive.read("two.DDS").ok().as_deref(),
            Some(b"xyz".as_slice())
        );
        assert_eq!(
            archive.read("one.dds").ok().as_deref(),
            Some(b"abc".as_slice())
        );
        assert!(archive.read("three.dds").is_err());
    }

    /// No game, no art, and no panic.
    #[test]
    fn a_folder_without_the_game_gives_no_art() {
        let art = GameArt::read(Path::new("/nowhere/at/all"));
        assert!(art.backdrop.is_none());
        assert!(art.fans.is_none());
    }
}
