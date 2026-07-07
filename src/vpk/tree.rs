use anyhow::{anyhow, Result};
use std::collections::HashMap;

use super::format::{ArchiveLocation, FileEntry, ENTRY_TERMINATOR};

/// Serializes the directory tree. Embedded entries have their payload
/// appended to the returned data buffer and their `offset` updated to
/// point into it.
pub fn build_tree_and_embedded(entries: &mut [FileEntry]) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut tree: HashMap<String, HashMap<String, Vec<usize>>> = HashMap::new();

    for (i, e) in entries.iter().enumerate() {
        tree.entry(e.path.ext.clone())
            .or_default()
            .entry(e.path.dir.clone())
            .or_default()
            .push(i);
    }

    let mut dir_buf: Vec<u8> = Vec::new();
    let mut data_buf: Vec<u8> = Vec::new();
    let mut offset_writebacks: Vec<(usize, u32)> = Vec::new();

    // Extensions, directories, and stems are sorted for deterministic output.
    let mut exts: Vec<String> = tree.keys().cloned().collect();
    exts.sort_unstable();

    for ext in &exts {
        let Some(dirs) = tree.get(ext) else { continue };

        write_cstring(&mut dir_buf, ext);

        let mut dir_keys: Vec<String> = dirs.keys().cloned().collect();
        dir_keys.sort_unstable();

        for dir in &dir_keys {
            let Some(files) = dirs.get(dir) else { continue };

            write_cstring(&mut dir_buf, dir);

            let mut sorted_indices = files.clone();
            sorted_indices.sort_by(|&a, &b| entries[a].path.stem.cmp(&entries[b].path.stem));

            for &idx in &sorted_indices {
                let e = &entries[idx];
                write_cstring(&mut dir_buf, &e.path.stem);

                dir_buf.extend_from_slice(&e.crc32.to_le_bytes());
                dir_buf.extend_from_slice(&0u16.to_le_bytes());
                dir_buf.extend_from_slice(&e.location.to_index().to_le_bytes());

                match e.location {
                    ArchiveLocation::Embedded => {
                        // The entry's end, not just its start, must stay
                        // addressable within u32 offsets.
                        if data_buf.len() + e.data.len() > u32::MAX as usize {
                            return Err(anyhow!(
                                "Total data size exceeds 4 GB limit for single VPK file"
                            ));
                        }
                        let offset = data_buf.len() as u32;
                        dir_buf.extend_from_slice(&offset.to_le_bytes());
                        dir_buf.extend_from_slice(&e.size.to_le_bytes());
                        data_buf.extend_from_slice(&e.data);
                        offset_writebacks.push((idx, offset));
                    }
                    ArchiveLocation::Chunk(_) => {
                        dir_buf.extend_from_slice(&e.offset.to_le_bytes());
                        dir_buf.extend_from_slice(&e.size.to_le_bytes());
                    }
                }

                dir_buf.extend_from_slice(&ENTRY_TERMINATOR.to_le_bytes());
            }

            dir_buf.push(0);
        }

        dir_buf.push(0);
    }

    dir_buf.push(0);

    for (idx, offset) in offset_writebacks {
        entries[idx].offset = offset;
    }

    Ok((dir_buf, data_buf))
}

fn write_cstring(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(s.as_bytes());
    buf.push(0);
}
