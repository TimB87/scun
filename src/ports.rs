use crate::*;
use once_cell::sync::Lazy;
use std::fs;
use std::io::{BufRead, BufReader};
use std::time::UNIX_EPOCH;

pub type PackageInfo = (String, Option<String>);

use std::sync::RwLock;

pub static INSTALLED_PACKAGES: Lazy<RwLock<Vec<PackageInfo>>> = Lazy::new(|| {
    RwLock::new({
        let db_mod_time = fs::metadata("/var/lib/pkg/db")
            .map_err(|e| Box::new(e) as Box<dyn Error>)
            .and_then(|metadata| {
                metadata
                    .modified()
                    .map_err(|e| Box::new(e) as Box<dyn Error>)
            })
            .and_then(|mod_time| {
                mod_time
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| Box::new(e) as Box<dyn Error>)
            })
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        match read_cache_from_file(&CACHE_FILE_PATH) {
            Ok(contents) if is_cache_valid(&contents) => contents.data,
            _ => fetch_installed_packages(db_mod_time).unwrap_or_else(|e| {
                eprintln!("Error fetching installed packages: {e}");
                Vec::new()
            }),
        }
    })
});

fn fetch_installed_packages(db_mod_time: u64) -> Result<Vec<PackageInfo>, Box<dyn Error>> {
    let packages = list_installed_packages("/var/lib/pkg/db")?;
    save_cache_to_file(&CACHE_FILE_PATH, &packages, db_mod_time)?;
    Ok(packages)
}

pub fn find_ports_in_repositories(
    package_name: &str,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let file = File::open("/etc/prt-get.conf")?;
    let reader = BufReader::new(file);

    reader
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| {
            line.strip_prefix("prtdir ")
                .map(|stripped| stripped.trim().to_string())
        })
        .find_map(|repo_name| {
            let repo_path = format!("/{repo_name}/{package_name}");
            if Path::new(&repo_path).exists() {
                Some(repo_name[11..].to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| "N/A".to_string().into())
}

pub fn extract_pkgfile_version(port_dir: &str) -> Option<String> {
    let pkgfile_path = format!("{port_dir}/Pkgfile");
    let pkgfile_content = std::fs::read_to_string(pkgfile_path).ok()?;

    let mut version = None;
    let mut release = None;

    for line in pkgfile_content.lines() {
        if line.starts_with("version=") {
            version = line.split('=').nth(1)?.trim().to_string().into();
        } else if line.starts_with("release=") {
            release = line.split('=').nth(1)?.trim().to_string().into();
        }
        if version.is_some() && release.is_some() {
            break;
        }
    }

    version.and_then(|v| release.map(|r| format!("{v}-{r}")))
}

fn list_installed_packages(filename: &str) -> Result<Vec<PackageInfo>, CacheError> {
    let file = File::open(filename)?;
    let reader = BufReader::new(file);

    let mut packages = Vec::new();
    let mut lines_iter = reader.lines().peekable();

    while let Some(Ok(name)) = lines_iter.next() {
        if name.trim().is_empty() {
            continue;
        }

        if let Some(Ok(version)) = lines_iter.next() {
            packages.push((name, Some(version)));

            while lines_iter
                 .peek().is_some_and(|line| !line.as_ref().unwrap().trim().is_empty())
            {
                lines_iter.next(); // ignore footprint
            }
        }
    }

    Ok(packages)
}
