use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

pub fn xdg_config_dir() -> PathBuf {
    env_dir("XDG_CONFIG_HOME")
        .unwrap_or_else(|| home().join(".config"))
        .join("lda")
}

pub fn xdg_data_dir() -> PathBuf {
    env_dir("XDG_DATA_HOME")
        .unwrap_or_else(|| home().join(".local").join("share"))
        .join("lda")
}

pub fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o700);
    fs::set_permissions(path, perms)?;
    Ok(())
}

pub fn tighten_file(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)?;
    Ok(())
}

fn env_dir(key: &str) -> Option<PathBuf> {
    let value = std::env::var_os(key)?;
    if value.is_empty() {
        return None;
    }
    Some(PathBuf::from(value))
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn read_to_string_limited(path: &Path, max: usize) -> Result<String> {
    let file = fs::File::open(path)?;
    let mut reader = std::io::Read::take(file, max as u64);
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut buf)?;
    if buf.contains(&0) {
        return Err(Error::InvalidInput(format!(
            "{} contains a NUL byte",
            path.display()
        )));
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}
