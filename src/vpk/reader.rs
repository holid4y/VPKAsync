use anyhow::{bail, Context, Result};

use super::format::{ArchiveLocation, VpkHeader, EMPTY_COMPONENT, ENTRY_TERMINATOR, HEADER_SIZE};

// crc(4) + preload(2) + archive_index(2) + offset(4) + size(4) + terminator(2)
const ENTRY_METADATA_SIZE: usize = 18;

pub struct ParsedEntry {
    pub ext: String,
    pub dir: String,
    pub stem: String,
    pub crc32: u32,
    pub preload_bytes: u16,
    /// Inline data stored in the tree after the metadata. VPKAsync never
    /// writes preload data, but other tools' VPKs may contain it.
    pub preload: Vec<u8>,
    pub location: ArchiveLocation,
    pub offset: u32,
    pub size: u32,
}

impl ParsedEntry {
    pub fn full_path(&self) -> String {
        let mut out = String::new();
        if self.dir != EMPTY_COMPONENT {
            out.push_str(&self.dir);
            out.push('/');
        }
        if self.stem != EMPTY_COMPONENT {
            out.push_str(&self.stem);
        }
        if self.ext != EMPTY_COMPONENT {
            out.push('.');
            out.push_str(&self.ext);
        }
        out
    }
}

pub struct ParsedVpk {
    /// Byte offset where embedded data begins (header + tree).
    pub data_start: usize,
    pub entries: Vec<ParsedEntry>,
}

/// Parses the header and full directory tree of a VPK v1 file.
pub fn parse_dir_vpk(bytes: &[u8]) -> Result<ParsedVpk> {
    let header = VpkHeader::from_bytes(bytes)?;

    let tree_end = HEADER_SIZE
        .checked_add(header.tree_size as usize)
        .filter(|&end| end <= bytes.len())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "tree size {} exceeds file size {}",
                header.tree_size,
                bytes.len()
            )
        })?;

    let mut pos = HEADER_SIZE;
    let mut entries = Vec::new();

    loop {
        let ext = read_cstring(bytes, &mut pos, tree_end)?;
        if ext.is_empty() {
            break;
        }
        loop {
            let dir = read_cstring(bytes, &mut pos, tree_end)?;
            if dir.is_empty() {
                break;
            }
            loop {
                let stem = read_cstring(bytes, &mut pos, tree_end)?;
                if stem.is_empty() {
                    break;
                }
                entries.push(read_entry_metadata(
                    bytes, &mut pos, tree_end, &ext, &dir, stem,
                )?);
            }
        }
    }

    if pos != tree_end {
        bail!("tree ends at byte {pos} but the header declares {tree_end}");
    }

    Ok(ParsedVpk {
        data_start: tree_end,
        entries,
    })
}

fn read_cstring(bytes: &[u8], pos: &mut usize, end: usize) -> Result<String> {
    let start = *pos;
    while *pos < end && bytes[*pos] != 0 {
        *pos += 1;
    }
    if *pos >= end {
        bail!("unterminated string at byte {start}");
    }
    let s = std::str::from_utf8(&bytes[start..*pos])
        .with_context(|| format!("invalid UTF-8 in tree string at byte {start}"))?
        .to_string();
    *pos += 1;
    Ok(s)
}

fn read_entry_metadata(
    bytes: &[u8],
    pos: &mut usize,
    end: usize,
    ext: &str,
    dir: &str,
    stem: String,
) -> Result<ParsedEntry> {
    if *pos + ENTRY_METADATA_SIZE > end {
        bail!("truncated entry metadata for {dir}/{stem}.{ext}");
    }

    let b = &bytes[*pos..*pos + ENTRY_METADATA_SIZE];
    let crc32 = u32::from_le_bytes(b[0..4].try_into().unwrap());
    let preload_bytes = u16::from_le_bytes(b[4..6].try_into().unwrap());
    let archive_index = u16::from_le_bytes(b[6..8].try_into().unwrap());
    let offset = u32::from_le_bytes(b[8..12].try_into().unwrap());
    let size = u32::from_le_bytes(b[12..16].try_into().unwrap());
    let terminator = u16::from_le_bytes(b[16..18].try_into().unwrap());

    if terminator != ENTRY_TERMINATOR {
        bail!("bad entry terminator {terminator:#06x} for {dir}/{stem}.{ext}");
    }

    *pos += ENTRY_METADATA_SIZE;

    let preload_end = *pos + preload_bytes as usize;
    if preload_end > end {
        bail!("truncated preload data for {dir}/{stem}.{ext}");
    }
    let preload = bytes[*pos..preload_end].to_vec();
    *pos = preload_end;

    Ok(ParsedEntry {
        ext: ext.to_string(),
        dir: dir.to_string(),
        stem,
        crc32,
        preload_bytes,
        preload,
        location: ArchiveLocation::from_index(archive_index),
        offset,
        size,
    })
}
