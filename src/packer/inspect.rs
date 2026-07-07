use anyhow::{Context, Result};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use crate::vpk::format::ArchiveLocation;
use crate::vpk::reader;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VpkMode {
    SingleFile,
    MultiChunk,
}

impl VpkMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::SingleFile => "single-file",
            Self::MultiChunk => "multi-chunk",
        }
    }
}

pub struct VpkInspection {
    pub mode: VpkMode,
    pub file_count: usize,
    pub chunk_count: usize,
    pub total_data_bytes: u64,
    /// Stored paths in tree order.
    pub paths: Vec<String>,
    pub problems: Vec<String>,
}

/// Parses an existing VPK (single-file or `_dir.vpk`). With `check_data`,
/// also loads the referenced payload and CRC-checks every entry.
pub fn inspect_vpk(vpk_path: &Path, check_data: bool) -> Result<VpkInspection> {
    let bytes =
        fs::read(vpk_path).with_context(|| format!("could not read {}", vpk_path.display()))?;

    let parsed = reader::parse_dir_vpk(&bytes).with_context(|| {
        let name = vpk_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if looks_like_chunk_file(name) {
            format!(
                "could not parse {}: chunk files hold raw data without a header; \
                 point at the matching _dir.vpk instead",
                vpk_path.display()
            )
        } else {
            format!("could not parse {}", vpk_path.display())
        }
    })?;

    let mut problems = Vec::new();
    let mut referenced_chunks: BTreeSet<u16> = BTreeSet::new();
    let mut embedded_count = 0usize;

    for entry in &parsed.entries {
        match entry.location {
            ArchiveLocation::Embedded => embedded_count += 1,
            ArchiveLocation::Chunk(index) => {
                referenced_chunks.insert(index);
            }
        }
    }

    if embedded_count > 0 && !referenced_chunks.is_empty() {
        problems.push(format!(
            "tree mixes {embedded_count} embedded entries with chunk-archive entries"
        ));
    }

    let mode = if referenced_chunks.is_empty() {
        VpkMode::SingleFile
    } else {
        VpkMode::MultiChunk
    };

    let mut chunks: HashMap<u16, Vec<u8>> = HashMap::new();
    if check_data && !referenced_chunks.is_empty() {
        match chunk_base(vpk_path) {
            Some(base) => {
                for &index in &referenced_chunks {
                    let chunk_path = PathBuf::from(format!("{}_{:03}.vpk", base.display(), index));
                    match fs::read(&chunk_path) {
                        Ok(data) => {
                            chunks.insert(index, data);
                        }
                        Err(e) => {
                            problems.push(format!("missing chunk {}: {}", chunk_path.display(), e))
                        }
                    }
                }
            }
            None => problems.push(
                "entries reference chunk archives, but the file name does not end in \
                 '_dir.vpk', so chunk files cannot be located"
                    .to_string(),
            ),
        }
    }

    let mut total_data_bytes = 0u64;
    let mut paths = Vec::with_capacity(parsed.entries.len());

    for entry in &parsed.entries {
        paths.push(entry.full_path());
        total_data_bytes += entry.preload.len() as u64 + entry.size as u64;

        if !check_data {
            continue;
        }

        // (source bytes, base offset of the data area within them)
        let source = match entry.location {
            ArchiveLocation::Embedded => Some((bytes.as_slice(), parsed.data_start)),
            ArchiveLocation::Chunk(index) => chunks.get(&index).map(|chunk| (chunk.as_slice(), 0)),
        };
        let Some((source, base)) = source else {
            continue; // missing chunk, already reported once
        };

        let stored = base
            .checked_add(entry.offset as usize)
            .and_then(|start| Some((start, start.checked_add(entry.size as usize)?)))
            .and_then(|(start, end)| source.get(start..end));

        match stored {
            Some(data) => {
                let mut hasher = crc32fast::Hasher::new();
                hasher.update(&entry.preload);
                hasher.update(data);
                if hasher.finalize() != entry.crc32 {
                    problems.push(format!(
                        "{}: CRC32 mismatch (stored data is corrupted)",
                        entry.full_path()
                    ));
                }
            }
            None => problems.push(format!(
                "{}: offset {} + size {} is out of range",
                entry.full_path(),
                entry.offset,
                entry.size
            )),
        }
    }

    Ok(VpkInspection {
        mode,
        file_count: parsed.entries.len(),
        chunk_count: referenced_chunks.len(),
        total_data_bytes,
        paths,
        problems,
    })
}

fn chunk_base(dir_path: &Path) -> Option<PathBuf> {
    let name = dir_path.file_name()?.to_str()?;
    let base_name = name.strip_suffix("_dir.vpk")?;
    if base_name.is_empty() {
        return None;
    }
    Some(dir_path.with_file_name(base_name))
}

fn looks_like_chunk_file(name: &str) -> bool {
    name.strip_suffix(".vpk")
        .and_then(|s| s.rsplit_once('_'))
        .map(|(_, digits)| digits.len() == 3 && digits.chars().all(|c| c.is_ascii_digit()))
        .unwrap_or(false)
}
