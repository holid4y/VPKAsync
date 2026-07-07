use anyhow::{bail, Result};

pub const VPK_SIGNATURE: u32 = 0x55AA1234;
pub const VPK_VERSION: u32 = 1;
pub const HEADER_SIZE: usize = 12;

// VPK v1 uses 0x7FFF for data embedded in the directory archive.
pub const EMBEDDED_ARCHIVE_INDEX: u16 = 0x7FFF;
pub const ENTRY_TERMINATOR: u16 = 0xFFFF;

// Empty ext/dir/stem are stored as a single space; an empty string would
// serialize as a bare NUL, which the tree parser reads as a section end.
pub const EMPTY_COMPONENT: &str = " ";

pub const SINGLE_VPK_LIMIT: u64 = 600 * 1024 * 1024;
pub const CHUNK_SIZE: u64 = 600 * 1024 * 1024;

pub struct VpkHeader {
    pub signature: u32,
    pub version: u32,
    pub tree_size: u32,
}

impl VpkHeader {
    pub fn new(tree_size: u32) -> Self {
        Self {
            signature: VPK_SIGNATURE,
            version: VPK_VERSION,
            tree_size,
        }
    }

    pub fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut bytes = [0u8; HEADER_SIZE];
        bytes[0..4].copy_from_slice(&self.signature.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.version.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.tree_size.to_le_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_SIZE {
            bail!("VPK header truncated ({} bytes)", bytes.len());
        }
        let signature = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let tree_size = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        if signature != VPK_SIGNATURE {
            bail!("invalid VPK signature {signature:#010x}");
        }
        if version != VPK_VERSION {
            bail!("unsupported VPK version {version}");
        }
        Ok(Self {
            signature,
            version,
            tree_size,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArchiveLocation {
    Embedded,
    Chunk(u16),
}

impl ArchiveLocation {
    pub fn to_index(self) -> u16 {
        match self {
            Self::Embedded => EMBEDDED_ARCHIVE_INDEX,
            Self::Chunk(index) => index,
        }
    }

    pub fn from_index(index: u16) -> Self {
        if index == EMBEDDED_ARCHIVE_INDEX {
            Self::Embedded
        } else {
            Self::Chunk(index)
        }
    }
}

/// Path components as stored in the tree: lowercased, `/` separators,
/// empty components replaced by [`EMPTY_COMPONENT`]. `full_path` is the
/// human-readable form without sentinels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedVpkPath {
    pub ext: String,
    pub dir: String,
    pub stem: String,
    pub full_path: String,
}

pub struct FileEntry {
    pub path: NormalizedVpkPath,
    pub data: Vec<u8>,
    pub crc32: u32,
    pub location: ArchiveLocation,
    pub offset: u32,
    pub size: u32,
}
