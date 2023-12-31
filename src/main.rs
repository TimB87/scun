use lazy_static::lazy_static;
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
            let prtdir = line.trim();
            // `prtdir ` is 7 characters long
            prtdirs.push(prtdir[7..].to_string());
        }
    }

    let repository = "N/A".to_string();

    for repo_name in prtdirs {
        let repo_path = format!("/{repo_name}/{package_name}");
        if Path::new(&repo_path).exists() {
            // `/usr/ports/` is 11 character long
            return Ok(repo_name[11..].to_string());
        }
    }

    Ok(repository)
}

lazy_static! {
    static ref VERSION_REGEX: Regex = Regex::new(r#"version=(.+)"#).unwrap();
    static ref RELEASE_REGEX: Regex = Regex::new(r#"release=(.+)"#).unwrap();
}

fn extract_pkgfile_version(port_dir: &str) -> Option<String> {
    let pkgfile_path = format!("{port_dir}/Pkgfile");
    if !Path::new(&pkgfile_path).exists() {
        return None;
    }

    let pkgfile_content = std::fs::read_to_string(pkgfile_path).ok()?;

    let version = match VERSION_REGEX.captures(&pkgfile_content) {
        Some(captures) => captures.get(1).map(|m| m.as_str()).unwrap_or(""),
        None => {
            eprintln!("Failed to extract version from Pkgfile for: {port_dir}");
            ""
        }
    };
    let release = RELEASE_REGEX
        .captures(&pkgfile_content)?
        .get(1)
        .map(|m| m.as_str())
        .unwrap_or("");

    Some(format!("{version}-{release}"))
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
            let package = current_package
                .as_mut()
                .expect("Error: no package was evaluated");
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
    if let Err(e) = libnotify::init("scun") {
        eprintln!("Failed to initialize libnotify: {}", e);
        return Err(e.into());
    }
    let notification_body: String = output.join("\n");
    let n = libnotify::Notification::new("Port Updates", &notification_body as &str, None);
    n.set_timeout(5000);
    if let Err(e) = n.show() {
        eprintln!("Failed to show notification: {}", e);
        return Err(e.into());
    }
    libnotify::uninit();

    Ok(())
}

fn print_mode(output: Vec<String>, submode: Option<String>) -> Result<(), Box<dyn Error>> {
    if let Some(sub) = submode {
        if sub == "-i" || sub == "--icon" {
            println!("󰚰 {}", output.len() - 1);
        } else if sub == "-l" || sub == "--long" {
            for line in output {
                println!("{line}");
            }
        } else {
            println!("{}", output.len() - 1);
        }
    } else {
        println!("{}", output.len() - 1);
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let packages = list_installed_packages("/var/lib/pkg/db")?;
    let mut output = Vec::new();

    output.push(format!(
        "{:<20} {:<15} {:<15}",
        "Port", "Version", "Available"
    ));

    for (name, version) in &packages {
        let repository = find_ports_in_repositories(name)?;
        let available_version = extract_pkgfile_version(&format!("/usr/ports/{repository}/{name}"))
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
        let message = r#"Usage: scun [notify|print]
notify: send a list via libnotify
print: prints the number of available updates
    --icon|-i: adds an icon
    --long|-l: prints the whole list"#;
        eprintln!("{message}");
        process::exit(1);
    }

    let mode = &args[1];
    let submode = match args.len() {
        0..=2 => Option::None,
        3 => Some(args[2].to_string()),
        _ => Option::None,
    };

    if mode == "notify" || mode == "n" {
        notify_mode(output)?;
    } else if mode == "print" || mode == "p" {
        print_mode(output, submode)?;
    } else {
        eprintln!("Invalid mode: {mode}. Use 'notify' or 'print'.");
        process::exit(1);
    }

    Ok(())
}
