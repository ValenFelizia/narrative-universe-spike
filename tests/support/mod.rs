use std::path::{Path, PathBuf};

use tempfile::TempDir;

pub struct Scratch {
    _dir: TempDir,
    pub path: PathBuf,
}

pub fn scratch(name: &str) -> Scratch {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join(name);
    Scratch { _dir: dir, path }
}

pub fn bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("read bytes")
}
