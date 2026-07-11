use crate::ports::PackageInfo;
use core::fmt;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize)]
pub struct CacheData {
    pub data: Vec<PackageInfo>,
    timestamp: Option<u64>,
    db_mod_time: Option<u64>,
}

#[derive(Debug)]
pub enum CacheError {
    Io(io::Error),
    Serde(serde_json::Error),
    SystemTime(std::time::SystemTimeError),
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CacheError::Io(e) => write!(f, "IO error: {e}"),
            CacheError::Serde(e) => write!(f, "Serialization error: {e}"),
            CacheError::SystemTime(e) => write!(f, "System time error: {e}"),
        }
    }
}

impl Error for CacheError {}

impl From<io::Error> for CacheError {
    fn from(error: io::Error) -> Self {
        CacheError::Io(error)
    }
}

impl From<serde_json::Error> for CacheError {
    fn from(error: serde_json::Error) -> Self {
        CacheError::Serde(error)
    }
}

impl From<std::time::SystemTimeError> for CacheError {
    fn from(error: std::time::SystemTimeError) -> Self {
        CacheError::SystemTime(error)
    }
}

pub static CACHE_FILE_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    xdg::BaseDirectories::new()
        .place_cache_file("scun.json")
        .expect("Failed to create cache file path")
});

pub fn save_cache_to_file(
    cache_path: &Path,
    data: &[PackageInfo],
    db_mod_time: u64,
) -> Result<(), CacheError> {
    let cache_data = CacheData {
        data: data.to_owned(),
        timestamp: Some(current_timestamp()?),
        db_mod_time: Some(db_mod_time),
    };

    let file = File::create(cache_path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, &cache_data)?;
    writer.flush()?;
    Ok(())
}

fn current_timestamp() -> Result<u64, CacheError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

pub fn is_cache_valid(cache_data: &CacheData, db_mod_time: u64) -> bool {
    cache_data.db_mod_time == Some(db_mod_time)
}

pub fn read_cache_from_file(cache_path: &Path) -> Result<CacheData, CacheError> {
    let file = File::open(cache_path)?;
    let reader = BufReader::new(file);
    Ok(serde_json::from_reader(reader)?)
}
