use std::fs;
use std::path::{Path, PathBuf};

use crate::packer::inspect::{self, VpkMode};
use crate::packer::{self, multi, paths, single, validate, PackOptions};
use crate::vpk::format::{
    ArchiveLocation, FileEntry, EMBEDDED_ARCHIVE_INDEX, EMPTY_COMPONENT, ENTRY_TERMINATOR,
    HEADER_SIZE, VPK_SIGNATURE, VPK_VERSION,
};
use crate::vpk::reader;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("vpkasync_test_{}_{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_fixture(root: &Path) {
    fs::create_dir_all(root.join("Materials/Models")).unwrap();
    fs::write(
        root.join("Materials/Models/Test_Texture.VMT"),
        b"vmt contents",
    )
    .unwrap();
    fs::write(root.join("Materials/base.vtf"), vec![7u8; 2048]).unwrap();
    fs::write(root.join("README"), b"no extension").unwrap();
    fs::write(root.join("root.txt"), b"root file").unwrap();
}

fn options(single: bool, multi: bool) -> PackOptions {
    PackOptions {
        single,
        multi,
        threads: None,
    }
}

fn entry_with_data(name: &str, data: Vec<u8>) -> FileEntry {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&data);
    let size = data.len() as u32;
    FileEntry {
        path: paths::normalize_rel_path(Path::new(name)),
        crc32: hasher.finalize(),
        size,
        data,
        location: ArchiveLocation::Chunk(0),
        offset: 0,
    }
}

#[test]
fn single_vpk_round_trip() {
    let input = TempDir::new("single_in");
    let output = TempDir::new("single_out");
    write_fixture(input.path());

    let out_base = output.path().join("pack");
    let summary =
        packer::package_folder(input.path(), &out_base, options(true, false), |_| {}).unwrap();

    assert!(!summary.use_multi);
    assert!(summary.skipped.is_empty());
    assert_eq!(summary.generated_files.len(), 1);

    let vpk_path = out_base.with_extension("vpk");
    let bytes = fs::read(&vpk_path).unwrap();
    let parsed = reader::parse_dir_vpk(&bytes).unwrap();
    assert_eq!(parsed.entries.len(), 4);

    let entry = parsed
        .entries
        .iter()
        .find(|e| e.full_path() == "materials/models/test_texture.vmt")
        .expect("mixed-case path should be stored lowercased");
    assert_eq!(entry.location, ArchiveLocation::Embedded);
    let start = parsed.data_start + entry.offset as usize;
    assert_eq!(&bytes[start..start + entry.size as usize], b"vmt contents");

    let no_ext = parsed
        .entries
        .iter()
        .find(|e| e.stem == "readme")
        .expect("extensionless file should be present");
    assert_eq!(no_ext.ext, EMPTY_COMPONENT);
    assert_eq!(no_ext.dir, EMPTY_COMPONENT);
}

#[test]
fn multi_vpk_round_trip() {
    let input = TempDir::new("multi_in");
    let output = TempDir::new("multi_out");
    write_fixture(input.path());

    let out_base = output.path().join("pack");
    let summary =
        packer::package_folder(input.path(), &out_base, options(false, true), |_| {}).unwrap();

    assert!(summary.use_multi);

    let dir_bytes = fs::read(format!("{}_dir.vpk", out_base.display())).unwrap();
    let parsed = reader::parse_dir_vpk(&dir_bytes).unwrap();
    assert_eq!(parsed.entries.len(), 4);
    // _dir.vpk holds only header + tree, no payload.
    assert_eq!(parsed.data_start, dir_bytes.len());

    let chunk0 = fs::read(format!("{}_000.vpk", out_base.display())).unwrap();
    for entry in &parsed.entries {
        let ArchiveLocation::Chunk(index) = entry.location else {
            panic!("multi mode must not embed data");
        };
        assert_eq!(index, 0);

        let start = entry.offset as usize;
        let stored = &chunk0[start..start + entry.size as usize];
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(stored);
        assert_eq!(hasher.finalize(), entry.crc32);
    }
}

#[test]
fn deterministic_output_across_runs() {
    let input = TempDir::new("det_in");
    let output = TempDir::new("det_out");
    write_fixture(input.path());

    let out_base = output.path().join("pack");
    packer::package_folder(input.path(), &out_base, options(true, false), |_| {}).unwrap();
    let first = fs::read(out_base.with_extension("vpk")).unwrap();

    packer::package_folder(input.path(), &out_base, options(true, false), |_| {}).unwrap();
    let second = fs::read(out_base.with_extension("vpk")).unwrap();

    assert_eq!(first, second);
}

#[test]
fn first_fit_opens_new_chunk_when_full() {
    let mut entries = vec![
        entry_with_data("a.bin", vec![1; 6]),
        entry_with_data("b.bin", vec![2; 6]),
        entry_with_data("c.bin", vec![3; 4]),
    ];

    let chunks = multi::assign_chunks(&mut entries, 10).unwrap();

    assert_eq!(chunks.len(), 2);
    assert_eq!(entries[0].location, ArchiveLocation::Chunk(0));
    assert_eq!(entries[1].location, ArchiveLocation::Chunk(1));
    // First-fit: the third file (4 bytes) goes back into chunk 0.
    assert_eq!(entries[2].location, ArchiveLocation::Chunk(0));
    assert_eq!(entries[2].offset, 6);
    assert_eq!(chunks[0].len(), 10);
    assert_eq!(chunks[1].len(), 6);
}

#[test]
fn assign_chunks_rejects_sentinel_chunk_index() {
    // With chunk_size 1, every 1-byte file opens a new chunk; the 32768th
    // would land on index 0x7FFF, the embedded-data sentinel.
    let mut entries: Vec<FileEntry> = (0..32768)
        .map(|_| entry_with_data("f.bin", vec![0u8]))
        .collect();
    let err = multi::assign_chunks(&mut entries, 1).unwrap_err();
    assert!(err.to_string().contains("chunks"));
}

#[test]
fn stale_orphan_chunks_are_removed() {
    let input = TempDir::new("stale_in");
    let output = TempDir::new("stale_out");
    write_fixture(input.path());

    let out_base = output.path().join("pack");
    let orphan = PathBuf::from(format!("{}_007.vpk", out_base.display()));
    fs::write(&orphan, b"stale data").unwrap();

    packer::package_folder(input.path(), &out_base, options(false, true), |_| {}).unwrap();

    assert!(!orphan.exists(), "orphan chunk past a gap must be deleted");
    assert!(PathBuf::from(format!("{}_000.vpk", out_base.display())).exists());
}

#[cfg(unix)]
#[test]
fn warns_on_case_colliding_paths() {
    let input = TempDir::new("collide_in");
    let output = TempDir::new("collide_out");
    fs::write(input.path().join("Foo.txt"), b"one").unwrap();
    fs::write(input.path().join("foo.TXT"), b"two").unwrap();

    let mut logs = Vec::new();
    packer::package_folder(
        input.path(),
        &output.path().join("pack"),
        options(true, false),
        |line| logs.push(line),
    )
    .unwrap();

    assert!(
        logs.iter().any(|l| l.contains("both map to VPK path")),
        "expected a case-collision warning, got: {logs:?}"
    );
}

#[cfg(unix)]
#[test]
fn errors_when_no_files_are_readable() {
    use std::os::unix::fs::PermissionsExt;

    let input = TempDir::new("unreadable_in");
    let output = TempDir::new("unreadable_out");
    let file = input.path().join("secret.bin");
    fs::write(&file, b"data").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();

    let result = packer::package_folder(
        input.path(),
        &output.path().join("pack"),
        options(true, false),
        |_| {},
    );

    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    let err = match result {
        Ok(_) => panic!("all files unreadable must not write an empty VPK"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("could be read"));
}

#[cfg(unix)]
#[test]
fn warns_on_skipped_symlinks() {
    let input = TempDir::new("symlink_in");
    let output = TempDir::new("symlink_out");
    fs::write(input.path().join("real.txt"), b"real").unwrap();
    std::os::unix::fs::symlink(input.path().join("real.txt"), input.path().join("link.txt"))
        .unwrap();

    let mut logs = Vec::new();
    let out_base = output.path().join("pack");
    packer::package_folder(input.path(), &out_base, options(true, false), |line| {
        logs.push(line)
    })
    .unwrap();

    assert!(
        logs.iter()
            .any(|l| l.contains("symbolic links are not followed")),
        "expected a symlink warning, got: {logs:?}"
    );

    let bytes = fs::read(out_base.with_extension("vpk")).unwrap();
    let parsed = reader::parse_dir_vpk(&bytes).unwrap();
    assert_eq!(parsed.entries.len(), 1, "only the real file is packed");
}

#[test]
fn parser_rejects_bad_signature() {
    let bytes = vec![0u8; 64];
    assert!(reader::parse_dir_vpk(&bytes).is_err());
}

#[test]
fn parser_rejects_truncated_tree() {
    let mut entries = vec![entry_with_data("a.txt", b"hello".to_vec())];
    let output = TempDir::new("trunc_out");
    let vpk_path = single::write_single_vpk(&mut entries, &output.path().join("pack")).unwrap();

    let bytes = fs::read(&vpk_path).unwrap();
    assert!(reader::parse_dir_vpk(&bytes[..HEADER_SIZE + 4]).is_err());
}

#[test]
fn verify_detects_corrupted_payload() {
    let output = TempDir::new("verify_out");
    let mut entries = vec![entry_with_data("a.txt", b"hello world".to_vec())];
    let vpk_path = single::write_single_vpk(&mut entries, &output.path().join("pack")).unwrap();

    let mut bytes = fs::read(&vpk_path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    fs::write(&vpk_path, &bytes).unwrap();

    assert!(validate::verify_single_vpk(&vpk_path, &entries).is_err());
}

#[test]
fn inspect_reports_single_file_mode() {
    let input = TempDir::new("inspect_single_in");
    let output = TempDir::new("inspect_single_out");
    write_fixture(input.path());

    let out_base = output.path().join("pack");
    packer::package_folder(input.path(), &out_base, options(true, false), |_| {}).unwrap();

    let inspection = inspect::inspect_vpk(&out_base.with_extension("vpk"), true).unwrap();
    assert!(inspection.problems.is_empty(), "{:?}", inspection.problems);
    assert_eq!(inspection.mode, VpkMode::SingleFile);
    assert_eq!(inspection.file_count, 4);
    assert_eq!(inspection.chunk_count, 0);
    assert!(inspection
        .paths
        .contains(&"materials/models/test_texture.vmt".to_string()));
}

#[test]
fn inspect_verifies_chunks_and_detects_corruption() {
    let input = TempDir::new("inspect_multi_in");
    let output = TempDir::new("inspect_multi_out");
    write_fixture(input.path());

    let out_base = output.path().join("pack");
    packer::package_folder(input.path(), &out_base, options(false, true), |_| {}).unwrap();

    let dir_path = PathBuf::from(format!("{}_dir.vpk", out_base.display()));
    let chunk_path = PathBuf::from(format!("{}_000.vpk", out_base.display()));

    let inspection = inspect::inspect_vpk(&dir_path, true).unwrap();
    assert!(inspection.problems.is_empty(), "{:?}", inspection.problems);
    assert_eq!(inspection.mode, VpkMode::MultiChunk);
    assert_eq!(inspection.file_count, 4);
    assert_eq!(inspection.chunk_count, 1);

    let mut chunk = fs::read(&chunk_path).unwrap();
    chunk[0] ^= 0xFF;
    fs::write(&chunk_path, &chunk).unwrap();
    let inspection = inspect::inspect_vpk(&dir_path, true).unwrap();
    assert!(inspection
        .problems
        .iter()
        .any(|p| p.contains("CRC32 mismatch")));

    fs::remove_file(&chunk_path).unwrap();
    let inspection = inspect::inspect_vpk(&dir_path, true).unwrap();
    assert!(inspection
        .problems
        .iter()
        .any(|p| p.contains("missing chunk")));
}

fn synthetic_preload_vpk() -> Vec<u8> {
    let preload = b"abc";
    let archive_data = b"de";
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(preload);
    hasher.update(archive_data);
    let crc = hasher.finalize();

    let mut tree = Vec::new();
    for part in ["txt", " ", "a"] {
        tree.extend_from_slice(part.as_bytes());
        tree.push(0);
    }
    tree.extend_from_slice(&crc.to_le_bytes());
    tree.extend_from_slice(&(preload.len() as u16).to_le_bytes());
    tree.extend_from_slice(&EMBEDDED_ARCHIVE_INDEX.to_le_bytes());
    tree.extend_from_slice(&0u32.to_le_bytes());
    tree.extend_from_slice(&(archive_data.len() as u32).to_le_bytes());
    tree.extend_from_slice(&ENTRY_TERMINATOR.to_le_bytes());
    tree.extend_from_slice(preload);
    tree.extend_from_slice(&[0, 0, 0]); // end of stems, dirs, tree

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&VPK_SIGNATURE.to_le_bytes());
    bytes.extend_from_slice(&VPK_VERSION.to_le_bytes());
    bytes.extend_from_slice(&(tree.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&tree);
    bytes.extend_from_slice(archive_data);
    bytes
}

#[test]
fn dotted_folder_name_keeps_full_output_name() {
    let input = TempDir::new("dotted_in");
    let output = TempDir::new("dotted_out");
    write_fixture(input.path());

    let out_base = output.path().join("UEO_V1.02");
    let summary =
        packer::package_folder(input.path(), &out_base, options(true, false), |_| {}).unwrap();

    let expected = output.path().join("UEO_V1.02.vpk");
    assert_eq!(summary.generated_files, vec![expected.clone()]);
    assert!(expected.exists());
}

#[test]
fn parser_and_inspect_handle_preload_data() {
    let bytes = synthetic_preload_vpk();

    let parsed = reader::parse_dir_vpk(&bytes).unwrap();
    assert_eq!(parsed.entries.len(), 1);
    let entry = &parsed.entries[0];
    assert_eq!(entry.full_path(), "a.txt");
    assert_eq!(entry.preload, b"abc");
    assert_eq!(entry.size, 2);

    let output = TempDir::new("preload_out");
    let vpk_path = output.path().join("preload.vpk");
    fs::write(&vpk_path, &bytes).unwrap();

    let inspection = inspect::inspect_vpk(&vpk_path, true).unwrap();
    assert!(inspection.problems.is_empty(), "{:?}", inspection.problems);
    assert_eq!(inspection.file_count, 1);
    assert_eq!(inspection.total_data_bytes, 5);
}
