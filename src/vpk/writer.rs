use anyhow::{anyhow, Context, Result};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use super::format::VpkHeader;

/// Writes header + tree + optional embedded data to `path`.
pub fn write_vpk_file(path: &Path, tree: &[u8], embedded: &[u8]) -> Result<()> {
    let tree_size: u32 = tree
        .len()
        .try_into()
        .map_err(|_| anyhow!("Directory tree exceeds the 4 GB header limit"))?;

    let file =
        File::create(path).with_context(|| format!("could not create {}", path.display()))?;
    let mut writer = BufWriter::new(file);

    writer.write_all(&VpkHeader::new(tree_size).to_bytes())?;
    writer.write_all(tree)?;
    writer.write_all(embedded)?;
    writer.flush()?;

    Ok(())
}
