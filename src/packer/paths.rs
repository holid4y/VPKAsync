use std::path::Path;

use crate::vpk::format::{NormalizedVpkPath, EMPTY_COMPONENT};

/// Maps a path relative to the input folder onto the form stored in the
/// VPK tree. Source resolves VPK paths as lowercase lookup keys, so every
/// component is lowercased and separators are normalized to `/`.
pub fn normalize_rel_path(rel: &Path) -> NormalizedVpkPath {
    let ext = rel
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    let stem = rel
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    let dir = rel
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("")
        .replace('\\', "/")
        .to_lowercase();

    let mut full_path = String::new();
    if !dir.is_empty() {
        full_path.push_str(&dir);
        full_path.push('/');
    }
    full_path.push_str(&stem);
    if !ext.is_empty() {
        full_path.push('.');
        full_path.push_str(&ext);
    }

    NormalizedVpkPath {
        ext: with_sentinel(ext),
        dir: with_sentinel(dir),
        stem: with_sentinel(stem),
        full_path,
    }
}

fn with_sentinel(component: String) -> String {
    if component.is_empty() {
        EMPTY_COMPONENT.to_string()
    } else {
        component
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercases_all_components() {
        let p = normalize_rel_path(Path::new("Materials/Models/Test_Texture.VMT"));
        assert_eq!(p.ext, "vmt");
        assert_eq!(p.dir, "materials/models");
        assert_eq!(p.stem, "test_texture");
        assert_eq!(p.full_path, "materials/models/test_texture.vmt");
    }

    #[test]
    fn root_file_uses_dir_sentinel() {
        let p = normalize_rel_path(Path::new("readme.txt"));
        assert_eq!(p.ext, "txt");
        assert_eq!(p.dir, EMPTY_COMPONENT);
        assert_eq!(p.stem, "readme");
        assert_eq!(p.full_path, "readme.txt");
    }

    #[test]
    fn missing_extension_uses_ext_sentinel() {
        let p = normalize_rel_path(Path::new("materials/somefile"));
        assert_eq!(p.ext, EMPTY_COMPONENT);
        assert_eq!(p.dir, "materials");
        assert_eq!(p.stem, "somefile");
        assert_eq!(p.full_path, "materials/somefile");
    }

    #[test]
    fn dotfile_has_no_extension() {
        let p = normalize_rel_path(Path::new(".gitignore"));
        assert_eq!(p.ext, EMPTY_COMPONENT);
        assert_eq!(p.stem, ".gitignore");
        assert_eq!(p.full_path, ".gitignore");
    }

    #[test]
    fn backslashes_become_forward_slashes() {
        let p = normalize_rel_path(Path::new("Sound\\UI/Click.WAV"));
        assert_eq!(p.dir, "sound/ui");
        assert_eq!(p.full_path, "sound/ui/click.wav");
    }
}
