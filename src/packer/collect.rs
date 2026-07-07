use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use super::PackWarning;

pub struct CollectedFiles {
    pub files: Vec<PathBuf>,
    pub warnings: Vec<PackWarning>,
}

pub fn collect_files(root: &Path) -> CollectedFiles {
    let mut files = Vec::new();
    let mut warnings = Vec::new();

    for entry in WalkDir::new(root) {
        match entry {
            Ok(e) => {
                if e.path_is_symlink() {
                    // Symlinks are not followed; surface the omission
                    // instead of silently dropping the target.
                    warnings.push(PackWarning {
                        path: Some(e.into_path()),
                        message: "symbolic links are not followed".to_string(),
                    });
                } else if e.file_type().is_file() {
                    files.push(e.into_path());
                }
            }
            Err(e) => {
                let message = e
                    .io_error()
                    .map(|io| io.to_string())
                    .unwrap_or_else(|| e.to_string());
                warnings.push(PackWarning {
                    path: e.path().map(Path::to_path_buf),
                    message,
                });
            }
        }
    }

    CollectedFiles { files, warnings }
}

pub fn folder_total_size(root: &Path) -> u64 {
    WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| entry.metadata().ok())
        .map(|metadata| metadata.len())
        .sum()
}

/// Warns when the selected folder looks like a mod *subfolder* instead of
/// the mod root. VPKAsync packs the folder's contents, so selecting e.g.
/// `models/` produces stored paths without the `models/` prefix and TF2
/// will not find them.
pub fn folder_name_warning(root: &Path) -> Option<String> {
    let name = root.file_name()?.to_str()?;
    let lower = name.to_lowercase();
    if matches!(lower.as_str(), "models" | "materials" | "scripts" | "sound") {
        Some(format!(
            "Input folder is named '{name}'. VPKAsync packs the folder's contents, \
             so stored paths will not start with '{lower}/'. If this is a mod \
             subfolder, select the mod root instead."
        ))
    } else {
        None
    }
}

pub fn layout_warnings(root: &Path, files: &[PathBuf]) -> Vec<String> {
    let mut warnings = Vec::new();

    if let Some(warning) = folder_name_warning(root) {
        warnings.push(warning);
    }

    let mut has_model_files = false;
    let mut has_material_files = false;
    let mut has_models_root = false;
    let mut has_materials_root = false;

    for file in files {
        let Ok(rel) = file.strip_prefix(root) else {
            continue;
        };

        if let Some(ext) = rel.extension().and_then(|e| e.to_str()) {
            match ext.to_lowercase().as_str() {
                "mdl" | "vvd" | "vtx" | "phy" => has_model_files = true,
                "vmt" | "vtf" => has_material_files = true,
                _ => {}
            }
        }

        if let Some(first) = rel.components().next() {
            let first = first.as_os_str().to_string_lossy().to_lowercase();
            match first.as_str() {
                "models" => has_models_root = true,
                "materials" => has_materials_root = true,
                _ => {}
            }
        }
    }

    if has_model_files && !has_models_root {
        warnings.push(
            "Found model files (.mdl/.vvd/.vtx/.phy) but no top-level 'models/' folder. \
             TF2 loads models only from 'models/...' paths; the selected folder is \
             probably not the mod root."
                .to_string(),
        );
    }
    if has_material_files && !has_materials_root {
        warnings.push(
            "Found material files (.vmt/.vtf) but no top-level 'materials/' folder. \
             TF2 loads materials only from 'materials/...' paths; the selected folder \
             is probably not the mod root."
                .to_string(),
        );
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(root: &str, rels: &[&str]) -> Vec<PathBuf> {
        rels.iter().map(|r| Path::new(root).join(r)).collect()
    }

    #[test]
    fn warns_when_folder_is_named_models() {
        let root = Path::new("/mods/models");
        let files = paths("/mods/models", &["player/thing.mdl"]);
        let warnings = layout_warnings(root, &files);
        assert!(warnings.iter().any(|w| w.contains("named 'models'")));
        assert!(warnings.iter().any(|w| w.contains("top-level 'models/'")));
    }

    #[test]
    fn warns_on_materials_outside_materials_root() {
        let root = Path::new("/mods/my_mod");
        let files = paths("/mods/my_mod", &["custom/skin.vmt", "custom/skin.vtf"]);
        let warnings = layout_warnings(root, &files);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("top-level 'materials/'"));
    }

    #[test]
    fn no_warnings_for_proper_mod_root() {
        let root = Path::new("/mods/my_mod");
        let files = paths(
            "/mods/my_mod",
            &[
                "models/player/item.mdl",
                "models/player/item.dx90.vtx",
                "materials/models/player/item.vmt",
                "scripts/sounds.txt",
            ],
        );
        assert!(layout_warnings(root, &files).is_empty());
    }

    #[test]
    fn folder_name_check_is_case_insensitive() {
        assert!(folder_name_warning(Path::new("/mods/Materials")).is_some());
        assert!(folder_name_warning(Path::new("/mods/my_mod")).is_none());
    }
}
