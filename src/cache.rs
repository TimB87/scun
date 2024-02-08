use crate::*;
use core::fmt;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::io;
use std::io::Write;

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
            CacheError::Io(e) => write!(f, "IO error: {}", e),
            CacheError::Serde(e) => write!(f, "Serialization error: {}", e),
            CacheError::SystemTime(e) => write!(f, "System time error: {}", e),
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

pub static CACHE_FILE_PATH: Lazy<PathBuf> = Lazy::new(|| {
    xdg::BaseDirectories::new()
        .expect("Failed to create BaseDirectories")
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
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .ok(),
        db_mod_time: Some(db_mod_time),
    };

    let json = serde_json::to_string(&cache_data)?;
    let mut file = File::create(cache_path)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}

pub fn is_cache_valid(cache_data: &CacheData) -> bool {
    if let Ok(metadata) = std::fs::metadata("/var/lib/pkg/db") {
        if let Ok(mod_time) = metadata.modified() {
            if let Ok(mod_time_secs) = mod_time.duration_since(UNIX_EPOCH) {
                // Check if the stored modification time is older than the current modification time
                return cache_data
                    .db_mod_time
                    .map_or(false, |db_mod_time| db_mod_time >= mod_time_secs.as_secs());
            }
        }
    }
    false
}

pub fn read_cache_from_file(cache_path: &Path) -> Result<CacheData, Box<dyn Error>> {
    if let Ok(file) = File::open(cache_path) {
        let reader = BufReader::new(file);
        if let Ok(json) = serde_json::from_reader(reader) {
            return Ok(json);
        }
    }
    Err("Cache file not found or invalid".into())
}
