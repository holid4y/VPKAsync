use anyhow::{anyhow, Result};
use std::fs;
use std::path::{Path, PathBuf};

use super::validate;
use crate::vpk::format::{ArchiveLocation, FileEntry, CHUNK_SIZE, EMBEDDED_ARCHIVE_INDEX};
use crate::vpk::{tree, writer};

pub fn write_multi_vpk(entries: &mut [FileEntry], out_base: &Path) -> Result<Vec<PathBuf>> {
    remove_stale_outputs(out_base);

    let chunks = assign_chunks(entries, CHUNK_SIZE)?;

    let mut output_files = Vec::new();
    let mut chunk_paths = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let chunk_path = chunk_file_path(out_base, i);
        fs::write(&chunk_path, chunk)?;
        chunk_paths.push(chunk_path.clone());
        output_files.push(chunk_path);
    }

    let dir_path = dir_file_path(out_base);
    let (tree_bytes, _) = tree::build_tree_and_embedded(entries)?;
    writer::write_vpk_file(&dir_path, &tree_bytes, &[])?;

    validate::verify_multi_vpk(&dir_path, &chunk_paths, entries)?;

    output_files.push(dir_path);
    Ok(output_files)
}

/// First-fit placement in entry order: each file goes into the first chunk
/// with room, opening a new chunk only when none fits.
pub fn assign_chunks(entries: &mut [FileEntry], chunk_size: u64) -> Result<Vec<Vec<u8>>> {
    let mut chunks: Vec<Vec<u8>> = vec![Vec::new()];

    for e in entries.iter_mut() {
        let target = chunks
            .iter()
            .position(|c| c.len() as u64 + e.size as u64 <= chunk_size);

        let chunk_idx = match target {
            Some(idx) => idx,
            None => {
                chunks.push(Vec::new());
                chunks.len() - 1
            }
        };

        // Chunk indices at or above 0x7FFF collide with the embedded-data
        // sentinel and cannot be represented in VPK v1.
        if chunk_idx >= EMBEDDED_ARCHIVE_INDEX as usize {
            return Err(anyhow!(
                "input needs more than {} chunks, which VPK v1 cannot represent",
                EMBEDDED_ARCHIVE_INDEX
            ));
        }

        e.location = ArchiveLocation::Chunk(chunk_idx as u16);
        e.offset = chunks[chunk_idx].len() as u32;
        chunks[chunk_idx].extend_from_slice(&e.data);
    }

    Ok(chunks)
}

/// Removes every `{base}_NNN.vpk` from a previous run, including ones past
/// a numbering gap, so a run producing fewer chunks leaves no orphans that
/// look like part of the new set.
fn remove_stale_outputs(out_base: &Path) {
    let Some(base_name) = out_base.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let parent = match out_base.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };

    if let Ok(read_dir) = fs::read_dir(parent) {
        for entry in read_dir.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if is_chunk_file_name(name, base_name) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    let old_dir = dir_file_path(out_base);
    if old_dir.exists() {
        let _ = fs::remove_file(&old_dir);
    }
}

fn is_chunk_file_name(name: &str, base: &str) -> bool {
    name.strip_prefix(base)
        .and_then(|rest| rest.strip_prefix('_'))
        .and_then(|rest| rest.strip_suffix(".vpk"))
        .map(|digits| digits.len() >= 3 && digits.bytes().all(|b| b.is_ascii_digit()))
        .unwrap_or(false)
}

fn chunk_file_path(out_base: &Path, index: usize) -> PathBuf {
    PathBuf::from(format!("{}_{:03}.vpk", out_base.display(), index))
}

fn dir_file_path(out_base: &Path) -> PathBuf {
    PathBuf::from(format!("{}_dir.vpk", out_base.display()))
}
