pub mod collect;
pub mod inspect;
pub mod multi;
pub mod paths;
pub mod single;
pub mod validate;

use anyhow::{anyhow, Context, Result};
use crc32fast::Hasher;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::vpk::format::{ArchiveLocation, FileEntry, SINGLE_VPK_LIMIT};

// All file data is held in memory while packing; warn above this input size.
const LARGE_INPUT_WARNING: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Copy)]
pub struct PackOptions {
    pub single: bool,
    pub multi: bool,
    pub threads: Option<usize>,
}

pub struct PackWarning {
    pub path: Option<PathBuf>,
    pub message: String,
}

pub struct SkippedFile {
    pub path: PathBuf,
    pub reason: String,
}

pub struct PackSummary {
    pub generated_files: Vec<PathBuf>,
    pub skipped: Vec<SkippedFile>,
    pub bytes_read: u64,
    pub use_multi: bool,
}

pub fn default_output_base(input: &Path) -> PathBuf {
    input
        .file_name()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("output"))
}

pub fn package_folder<F>(
    input: &Path,
    out_base: &Path,
    options: PackOptions,
    mut log: F,
) -> Result<PackSummary>
where
    F: FnMut(String),
{
    let input = input
        .canonicalize()
        .with_context(|| format!("Could not open '{}'.", input.display()))?;

    if !input.is_dir() {
        return Err(anyhow!("'{}' is not a valid folder.", input.display()));
    }

    log(format!("Input: {}", input.display()));
    log(format!("Output base: {}", out_base.display()));
    log("[1/3] Collecting files...".to_string());

    let collected = collect::collect_files(&input);
    for warning in &collected.warnings {
        match &warning.path {
            Some(path) => log(format!(
                "Warning: Skipping {}: {}",
                path.display(),
                warning.message
            )),
            None => log(format!("Warning: Skipping: {}", warning.message)),
        }
    }

    let files = collected.files;
    if files.is_empty() {
        return Err(anyhow!("No files found in the selected folder."));
    }

    for warning in collect::layout_warnings(&input, &files) {
        log(format!("Warning: {warning}"));
    }

    // Two input files may normalize to the same stored path (e.g. Foo.txt
    // and foo.TXT on a case-sensitive filesystem); the engine will only
    // ever resolve one of them.
    let mut seen_paths: HashMap<String, &Path> = HashMap::new();
    for file in &files {
        if let Ok(rel) = file.strip_prefix(&input) {
            let normalized = paths::normalize_rel_path(rel).full_path;
            if let Some(previous) = seen_paths.insert(normalized.clone(), file.as_path()) {
                log(format!(
                    "Warning: '{}' and '{}' both map to VPK path '{}'; only one \
                     will be visible in-game.",
                    previous.display(),
                    file.display(),
                    normalized
                ));
            }
        }
    }

    let total_size: u64 = files
        .iter()
        .map(|f| fs::metadata(f).map(|m| m.len()).unwrap_or(0))
        .sum();

    if total_size > LARGE_INPUT_WARNING {
        log(format!(
            "Warning: input is {:.1} GB. VPKAsync holds all file data in memory \
             while packing; make sure enough free RAM is available.",
            total_size as f64 / 1024.0 / 1024.0 / 1024.0
        ));
    }

    let exceeds_practical_limit = total_size > SINGLE_VPK_LIMIT;

    let requested_multi = if options.single {
        false
    } else if options.multi {
        true
    } else {
        exceeds_practical_limit
    };

    let use_multi = requested_multi || exceeds_practical_limit;

    log(format!("{} files found", files.len()));
    log(format!(
        "Total size: {:.1} MB",
        total_size as f64 / 1024.0 / 1024.0
    ));
    if exceeds_practical_limit && options.single {
        log(format!(
            "Input is above {:.1} MB: forcing multi-chunk mode.",
            SINGLE_VPK_LIMIT as f64 / 1024.0 / 1024.0
        ));
    }
    log(format!(
        "Mode: {}",
        if use_multi {
            "multi-chunk (_dir + _000.vpk...)"
        } else {
            "single-file (.vpk)"
        }
    ));

    log("[2/3] Reading and processing files...".to_string());

    let skipped = Mutex::new(Vec::new());
    let bytes_read = AtomicU64::new(0);

    let read_entries = || read_all(&files, &input, &skipped, &bytes_read);

    let mut entries = if let Some(threads) = options.threads {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .context("Failed to create custom thread pool")?;
        pool.install(read_entries)
    } else {
        read_entries()
    };

    let skipped = skipped.into_inner().unwrap_or_default();
    if !skipped.is_empty() {
        log(format!(
            "{} file(s) were skipped due to errors.",
            skipped.len()
        ));
        for file in &skipped {
            log(format!("Skipped {}: {}", file.path.display(), file.reason));
        }
    }

    if entries.is_empty() {
        return Err(anyhow!(
            "None of the {} file(s) could be read; no VPK was written.",
            files.len()
        ));
    }

    let total_read = bytes_read.load(Ordering::Relaxed);
    log(format!(
        "Total read: {:.1} MB",
        total_read as f64 / 1024.0 / 1024.0
    ));

    log("[3/3] Writing VPK...".to_string());
    let generated_files = if use_multi {
        multi::write_multi_vpk(&mut entries, out_base)?
    } else {
        vec![single::write_single_vpk(&mut entries, out_base)?]
    };

    Ok(PackSummary {
        generated_files,
        skipped,
        bytes_read: total_read,
        use_multi,
    })
}

fn read_all(
    files: &[PathBuf],
    input: &Path,
    skipped: &Mutex<Vec<SkippedFile>>,
    bytes_read: &AtomicU64,
) -> Vec<FileEntry> {
    files
        .par_iter()
        .filter_map(|path| match read_one(path, input, bytes_read) {
            Ok(entry) => Some(entry),
            Err(reason) => {
                if let Ok(mut guard) = skipped.lock() {
                    guard.push(SkippedFile {
                        path: path.clone(),
                        reason,
                    });
                }
                None
            }
        })
        .collect()
}

fn read_one(path: &Path, input: &Path, bytes_read: &AtomicU64) -> Result<FileEntry, String> {
    let rel = path.strip_prefix(input).map_err(|e| e.to_string())?;

    // Per-file size is a u32 in VPK v1. Check the metadata first so an
    // oversized file is rejected without reading it into memory; re-check
    // after reading in case the file changed in between.
    let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
    if metadata.len() > u32::MAX as u64 {
        return Err(format!(
            "file is too large for VPK v1 (max {} bytes)",
            u32::MAX
        ));
    }

    let mut data = Vec::new();
    File::open(path)
        .and_then(|mut f| f.read_to_end(&mut data))
        .map_err(|e| e.to_string())?;

    if data.len() > u32::MAX as usize {
        return Err(format!(
            "file is too large for VPK v1 (max {} bytes)",
            u32::MAX
        ));
    }

    let mut hasher = Hasher::new();
    hasher.update(&data);
    let crc32 = hasher.finalize();

    bytes_read.fetch_add(data.len() as u64, Ordering::Relaxed);

    let size = data.len() as u32;
    Ok(FileEntry {
        path: paths::normalize_rel_path(rel),
        data,
        crc32,
        location: ArchiveLocation::Chunk(0),
        offset: 0,
        size,
    })
}
