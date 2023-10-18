use regex::Regex;
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process;

type PackageInfo = (String, Option<String>);

fn find_ports_in_repositories(package_name: &str) -> Result<String, Box<dyn Error>> {
    let file = File::open("/etc/prt-get.conf")?;
    let reader = BufReader::new(file);

    let mut prtdirs: Vec<String> = Vec::new();

    for line in reader.lines().flatten() {
        if line.starts_with("prtdir ") {
            let prtdir = line.trim().to_string();
            // `prtdir ` is 7 characters long
            prtdirs.push(prtdir[7..].to_string());
        }
    }

    let repository = "N/A".to_string();

    for repo_name in prtdirs {
        let repo_path = format!("/{}/{}", repo_name, package_name);
        if Path::new(&repo_path).exists() {
            // `/usr/ports/` is 11 character long
            return Ok(repo_name[11..].to_string());
        }
    }

    Ok(repository)
}

fn extract_pkgfile_version(port_dir: &str) -> Option<String> {
    let pkgfile_path = format!("{}/Pkgfile", port_dir);
    if !Path::new(&pkgfile_path).exists() {
        return None;
    }

    let pkgfile_content = std::fs::read_to_string(pkgfile_path).ok()?;
    let version_regex = Regex::new(r#"version=(.+)"#).ok()?;
    let release_regex = Regex::new(r#"release=(.+)"#).ok()?;

    let version = match version_regex.captures(&pkgfile_content) {
        Some(captures) => captures.get(1).map(|m| m.as_str()).unwrap_or(""),
        None => {
            eprintln!("Failed to extract version from Pkgfile for: {}", port_dir);
            ""
        }
    };
    let release = release_regex
        .captures(&pkgfile_content)?
        .get(1)
        .map(|m| m.as_str())
        .unwrap_or("");

    Some(format!("{}-{}", version, release))
}

fn list_installed_packages(filename: &str) -> Result<Vec<PackageInfo>, Box<dyn Error>> {
    let file = File::open(filename)?;
    let reader = BufReader::with_capacity(1024 * 1024, file);

    let mut packages = Vec::new();
    let mut current_package: Option<PackageInfo> = None;

    for line in reader.lines() {
        let line = line?;
        if line.is_empty() {
            if let Some(mut package) = current_package.take() {
                if let Some(available_version) =
                    extract_pkgfile_version(&format!("/usr/ports/{}", package.0))
                {
                    package.1 = Some(available_version);
                }
                packages.push(package);
            }
        } else if current_package.is_none() {
            current_package = Some((line, None));
        } else {
            let package = current_package.as_mut().unwrap();
            if package.1.is_none() {
                package.1 = Some(line);
            }
        }
    }

    if let Some(mut package) = current_package {
        if let Some(available_version) =
            extract_pkgfile_version(&format!("/usr/ports/{}", package.0))
        {
            package.1 = Some(available_version);
        }
        packages.push(package);
    }

    Ok(packages)
}

fn notify_mode(output: Vec<String>) -> Result<(), Box<dyn Error>> {
    libnotify::init("scun").unwrap();
    let notification_body: String = output.join("\n");
    let n = libnotify::Notification::new("Port Updates", &notification_body as &str, None);
    n.set_timeout(5000);
    n.show().unwrap();
    libnotify::uninit();

    Ok(())
}

fn print_mode(output: Vec<String>, submode: Option<String>) -> Result<(), Box<dyn Error>> {
    let icon: &str = match submode {
        Some(i) if i == "-i" || i == "--icon" => "󰚰 ",
        _ => "",
    };
    println!("{icon}{}", output.len() - 1);

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let packages = list_installed_packages("/var/lib/pkg/db")?;
    let mut output = Vec::new();

    output.push(format!(
        "{:<20} {:<15} {:<15}",
        "Port", "Version", "Available Version"
    ));

    for (name, version) in &packages {
        let repository = find_ports_in_repositories(name)?;
        let available_version =
            extract_pkgfile_version(&format!("/usr/ports/{}/{}", repository, name))
                .unwrap_or("N/A".to_string());

        if version.as_ref().map_or("unknown", |v| v) != available_version {
            output.push(format!(
                "{:<20} {:<15} {:<15}",
                name,
                version.as_ref().map_or("unknown", |v| v),
                available_version
            ));
        }
    }

    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: scun [notify|print]");
        process::exit(1);
    }

    let mode = &args[1];
    let submode = match args.len() {
        0..=2 => Option::None,
        3 => Some(args[2].to_string()),
        _ => Option::None,
    };

    if mode == "notify" {
        notify_mode(output)?;
    } else if mode == "print" {
        print_mode(output, submode)?;
    } else {
        eprintln!("Invalid mode: {}. Use 'notify' or 'print'.", mode);
        process::exit(1);
    }

    Ok(())
}
