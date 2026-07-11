use crate::cache::{
    is_cache_valid, read_cache_from_file, save_cache_to_file, CacheError, CACHE_FILE_PATH,
};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::UNIX_EPOCH;

pub type PackageInfo = (String, Option<String>);

const PKG_DB_PATH: &str = "/var/lib/pkg/db";
const PRT_GET_CONF_PATH: &str = "/etc/prt-get.conf";

static REPO_PATHS: LazyLock<Vec<PathBuf>> =
    LazyLock::new(|| match read_repository_paths(PRT_GET_CONF_PATH) {
        Ok(paths) => paths,
        Err(e) => {
            eprintln!("Failed to read {PRT_GET_CONF_PATH}: {e}");
            Vec::new()
        }
    });

pub static INSTALLED_PACKAGES: LazyLock<Vec<PackageInfo>> = LazyLock::new(|| {
    let db_mod_time = match pkg_db_mod_time() {
        Ok(db_mod_time) => db_mod_time,
        Err(e) => {
            eprintln!("Could not read package database metadata: {e}");
            return Vec::new();
        }
    };

    match read_cache_from_file(&CACHE_FILE_PATH) {
        Ok(contents) if is_cache_valid(&contents, db_mod_time) => contents.data,
        _ => fetch_installed_packages(db_mod_time).unwrap_or_else(|e| {
            eprintln!("Error fetching installed packages: {e}");
            Vec::new()
        }),
    }
});

fn read_repository_paths(path: &str) -> Result<Vec<PathBuf>, CacheError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    reader.lines().try_fold(Vec::new(), |mut paths, line| {
        if let Some(path) = parse_repository_path(&line?) {
            paths.push(path);
        }
        Ok(paths)
    })
}

fn parse_repository_path(line: &str) -> Option<PathBuf> {
    let line = line.split('#').next()?.trim();
    let mut fields = line.split_whitespace();

    match (fields.next(), fields.next()) {
        (Some("prtdir"), Some(path)) => Some(PathBuf::from(path)),
        _ => None,
    }
}

fn pkg_db_mod_time() -> Result<u64, CacheError> {
    Ok(fs::metadata(PKG_DB_PATH)?
        .modified()?
        .duration_since(UNIX_EPOCH)?
        .as_secs())
}

fn fetch_installed_packages(db_mod_time: u64) -> Result<Vec<PackageInfo>, CacheError> {
    let packages = list_installed_packages(PKG_DB_PATH)?;
    save_cache_to_file(&CACHE_FILE_PATH, &packages, db_mod_time)?;
    Ok(packages)
}

pub fn find_ports_in_repositories(package_name: &str) -> Option<PathBuf> {
    find_port_in_repositories(package_name, &REPO_PATHS)
}

fn find_port_in_repositories(package_name: &str, repo_paths: &[PathBuf]) -> Option<PathBuf> {
    repo_paths
        .iter()
        .map(|repo_path| repo_path.join(package_name))
        .find(|port_dir| port_dir.is_dir())
}

pub fn extract_pkgfile_version(port_dir: &Path) -> Option<String> {
    let pkgfile_path = port_dir.join("Pkgfile");
    let file = File::open(pkgfile_path).ok()?;
    let reader = BufReader::new(file);

    let mut version: Option<String> = None;
    let mut release: Option<String> = None;

    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim_start();
        if let Some(value) = line.strip_prefix("version=").map(str::trim) {
            version = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("release=").map(str::trim) {
            release = Some(value.to_string());
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
    let mut lines = reader.lines();

    while let Some(name) = next_non_empty_line(&mut lines)? {
        let Some(version) = lines.next().transpose()? else {
            break;
        };

        packages.push((name, Some(version)));
        skip_footprint(&mut lines)?;
    }

    Ok(packages)
}

fn next_non_empty_line<I>(lines: &mut I) -> Result<Option<String>, CacheError>
where
    I: Iterator<Item = io::Result<String>>,
{
    for line in lines.by_ref() {
        let line = line?;
        if !line.trim().is_empty() {
            return Ok(Some(line));
        }
    }

    Ok(None)
}

fn skip_footprint<I>(lines: &mut I) -> Result<(), CacheError>
where
    I: Iterator<Item = io::Result<String>>,
{
    for line in lines.by_ref() {
        if line?.trim().is_empty() {
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::io::Write;
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("test time is before unix epoch")
                .as_nanos();
            let path = env::temp_dir().join(format!("scun-{name}-{}-{nanos}", process::id()));
            fs::create_dir_all(&path).expect("failed to create test directory");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_file(path: &Path, contents: &str) {
        let mut file = File::create(path).expect("failed to create test file");
        file.write_all(contents.as_bytes())
            .expect("failed to write test file");
    }

    #[test]
    fn parse_repository_path_reads_prtdir_lines() {
        assert_eq!(
            parse_repository_path("  prtdir /usr/ports/core # comment"),
            Some(PathBuf::from("/usr/ports/core"))
        );
        assert_eq!(parse_repository_path("runscript /usr/ports/core"), None);
        assert_eq!(parse_repository_path("# prtdir /usr/ports/core"), None);
    }

    #[test]
    fn read_repository_paths_reads_only_prtdir_entries() {
        let temp = TestDir::new("prt-get-conf");
        let conf = temp.path().join("prt-get.conf");
        write_file(
            &conf,
            "\
prtdir /usr/ports/core
runscript /usr/ports/core
prtdir /usr/ports/contrib # comment
",
        );

        assert_eq!(
            read_repository_paths(conf.to_str().expect("test path is not valid utf-8"))
                .expect("failed to read repository paths"),
            vec![
                PathBuf::from("/usr/ports/core"),
                PathBuf::from("/usr/ports/contrib")
            ]
        );
    }

    #[test]
    fn find_port_in_repositories_preserves_repository_priority() {
        let temp = TestDir::new("ports-lookup");
        let first_repo = temp.path().join("first");
        let second_repo = temp.path().join("second");
        fs::create_dir_all(first_repo.join("foo")).expect("failed to create first foo");
        fs::create_dir_all(first_repo.join("bar")).expect("failed to create first bar");
        fs::create_dir_all(second_repo.join("foo")).expect("failed to create second foo");

        let repo_paths = [first_repo.clone(), second_repo.clone()];

        assert_eq!(
            find_port_in_repositories("foo", &repo_paths),
            Some(first_repo.join("foo"))
        );
        assert_eq!(
            find_port_in_repositories("bar", &repo_paths),
            Some(first_repo.join("bar"))
        );
        assert_eq!(find_port_in_repositories("missing", &repo_paths), None);
    }

    #[test]
    fn extract_pkgfile_version_reads_exact_headers() {
        let temp = TestDir::new("pkgfile");
        let port = temp.path().join("foo");
        fs::create_dir_all(&port).expect("failed to create port directory");
        write_file(
            &port.join("Pkgfile"),
            "\
# Description: fixture
version=1.2.3
release=4
",
        );

        assert_eq!(extract_pkgfile_version(&port), Some("1.2.3-4".to_string()));
    }

    #[test]
    fn list_installed_packages_reads_pkg_db_entries() {
        let temp = TestDir::new("pkg-db");
        let db = temp.path().join("db");
        write_file(
            &db,
            "\
foo
1.0-1
usr/bin/foo
usr/share/foo

bar
2.0-3
usr/bin/bar
",
        );

        assert_eq!(
            list_installed_packages(db.to_str().expect("test path is not valid utf-8"))
                .expect("failed to list installed packages"),
            vec![
                ("foo".to_string(), Some("1.0-1".to_string())),
                ("bar".to_string(), Some("2.0-3".to_string()))
            ]
        );
    }
}
