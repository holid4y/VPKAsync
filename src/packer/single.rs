use anyhow::Result;
use std::path::{Path, PathBuf};

use super::validate;
use crate::vpk::format::{ArchiveLocation, FileEntry};
use crate::vpk::{tree, writer};

/// Appends `.vpk` instead of `Path::with_extension`, which would replace
/// dot-suffixed folder names (`UEO_V1.02` becomes `UEO_V1.vpk`).
fn single_vpk_path(out_base: &Path) -> PathBuf {
    let name = out_base
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name.to_lowercase().ends_with(".vpk") {
        out_base.to_path_buf()
    } else {
        PathBuf::from(format!("{}.vpk", out_base.display()))
    }
}

pub fn write_single_vpk(entries: &mut [FileEntry], out_base: &Path) -> Result<PathBuf> {
    for e in entries.iter_mut() {
        e.location = ArchiveLocation::Embedded;
        e.offset = 0;
    }

    let out_path = single_vpk_path(out_base);
    let (tree_bytes, data_bytes) = tree::build_tree_and_embedded(entries)?;
    writer::write_vpk_file(&out_path, &tree_bytes, &data_bytes)?;

    validate::verify_single_vpk(&out_path, entries)?;

    Ok(out_path)
}
