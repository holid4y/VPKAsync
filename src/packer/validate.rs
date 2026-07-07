use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::vpk::format::{ArchiveLocation, FileEntry};
use crate::vpk::reader::{self, ParsedEntry, ParsedVpk};

/// Re-parses the written single-file VPK and checks every tree entry
/// against the packed data.
pub fn verify_single_vpk(vpk_path: &Path, entries: &[FileEntry]) -> Result<()> {
    let bytes = fs::read(vpk_path)
        .with_context(|| format!("verify: could not re-read {}", vpk_path.display()))?;
    let parsed = reader::parse_dir_vpk(&bytes)
        .with_context(|| format!("verify: could not parse {}", vpk_path.display()))?;

    let mut problems = Vec::new();
    for (parsed_entry, original) in match_entries(&parsed, entries, &mut problems) {
        match parsed_entry.location {
            ArchiveLocation::Embedded => {
                check_payload(
                    parsed_entry,
                    original,
                    &bytes,
                    parsed.data_start,
                    &mut problems,
                );
            }
            ArchiveLocation::Chunk(index) => problems.push(format!(
                "{}: expected embedded data, found archive index {index}",
                parsed_entry.full_path()
            )),
        }
    }

    report(vpk_path, problems)
}

/// Re-parses the written `_dir.vpk` and checks every tree entry against
/// the bytes stored in its chunk file.
pub fn verify_multi_vpk(
    dir_path: &Path,
    chunk_paths: &[PathBuf],
    entries: &[FileEntry],
) -> Result<()> {
    let dir_bytes = fs::read(dir_path)
        .with_context(|| format!("verify: could not re-read {}", dir_path.display()))?;
    let parsed = reader::parse_dir_vpk(&dir_bytes)
        .with_context(|| format!("verify: could not parse {}", dir_path.display()))?;

    let chunks: Vec<Vec<u8>> = chunk_paths
        .iter()
        .map(|p| fs::read(p).with_context(|| format!("verify: could not re-read {}", p.display())))
        .collect::<Result<_>>()?;

    let mut problems = Vec::new();
    for (parsed_entry, original) in match_entries(&parsed, entries, &mut problems) {
        match parsed_entry.location {
            ArchiveLocation::Embedded => problems.push(format!(
                "{}: unexpected embedded entry in multi-chunk mode",
                parsed_entry.full_path()
            )),
            ArchiveLocation::Chunk(index) => match chunks.get(index as usize) {
                Some(chunk) => {
                    check_payload(parsed_entry, original, chunk, 0, &mut problems);
                }
                None => problems.push(format!(
                    "{}: archive index {} out of range ({} chunks)",
                    parsed_entry.full_path(),
                    index,
                    chunks.len()
                )),
            },
        }
    }

    report(dir_path, problems)
}

type EntryKey<'a> = (&'a str, &'a str, &'a str, u32, u32);

fn original_key(e: &FileEntry) -> EntryKey<'_> {
    (
        e.path.ext.as_str(),
        e.path.dir.as_str(),
        e.path.stem.as_str(),
        e.crc32,
        e.size,
    )
}

fn parsed_key(p: &ParsedEntry) -> EntryKey<'_> {
    (
        p.ext.as_str(),
        p.dir.as_str(),
        p.stem.as_str(),
        p.crc32,
        p.size,
    )
}

/// Pairs each parsed tree entry with the original file it should contain.
/// Unmatched entries on either side are reported as problems.
fn match_entries<'a>(
    parsed: &'a ParsedVpk,
    entries: &'a [FileEntry],
    problems: &mut Vec<String>,
) -> Vec<(&'a ParsedEntry, &'a FileEntry)> {
    let mut originals: HashMap<EntryKey<'a>, Vec<&'a FileEntry>> = HashMap::new();
    for e in entries {
        originals.entry(original_key(e)).or_default().push(e);
    }

    let mut pairs = Vec::new();
    for p in &parsed.entries {
        match originals.get_mut(&parsed_key(p)).and_then(|v| v.pop()) {
            Some(original) => pairs.push((p, original)),
            None => problems.push(format!(
                "{}: tree entry does not match any packed file",
                p.full_path()
            )),
        }
    }

    for bucket in originals.values() {
        for e in bucket {
            problems.push(format!(
                "{}: missing from the written tree",
                e.path.full_path
            ));
        }
    }

    pairs
}

fn check_payload(
    parsed: &ParsedEntry,
    original: &FileEntry,
    source: &[u8],
    base: usize,
    problems: &mut Vec<String>,
) {
    if parsed.preload_bytes != 0 {
        problems.push(format!(
            "{}: unexpected preload bytes ({})",
            parsed.full_path(),
            parsed.preload_bytes
        ));
    }

    let range = base.checked_add(parsed.offset as usize).and_then(|start| {
        start
            .checked_add(parsed.size as usize)
            .map(|end| (start, end))
    });
    let Some((start, end)) = range else {
        problems.push(format!(
            "{}: offset {} + size {} overflows",
            parsed.full_path(),
            parsed.offset,
            parsed.size
        ));
        return;
    };

    let Some(stored) = source.get(start..end) else {
        problems.push(format!(
            "{}: offset {} + size {} exceeds archive size {}",
            parsed.full_path(),
            parsed.offset,
            parsed.size,
            source.len()
        ));
        return;
    };

    if stored != original.data.as_slice() {
        problems.push(format!(
            "{}: stored bytes at [{start}, {end}) do not match the original file",
            parsed.full_path()
        ));
    }
}

fn report(path: &Path, problems: Vec<String>) -> Result<()> {
    if problems.is_empty() {
        return Ok(());
    }

    for problem in &problems {
        eprintln!("verify FAIL: {problem}");
    }

    Err(anyhow!(
        "VPK self-check failed: {} problem(s) found in '{}'. \
         This is a VPKAsync bug — please report it.",
        problems.len(),
        path.display()
    ))
}
