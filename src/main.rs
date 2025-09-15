mod cache;
mod ports;

use cache::*;
use ports::*;

use std::cmp::Ordering;

use libversion::version_compare;
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

fn notify_mode(output: Vec<String>) -> Result<(), Box<dyn Error>> {
    if libnotify::init("scun").is_err() {
        return Err("Failed to initialize libnotify".into());
    }

    let notification_body = output.join("\n");
    let notification =
        libnotify::Notification::new("Port Updates", Some(notification_body.as_str()), None);

    notification.set_timeout(5000);
    notification.show()?;

    libnotify::uninit();
    Ok(())
}

fn print_mode(output: Vec<String>, submode: Option<String>) -> Result<(), Box<dyn Error>> {
    match submode.as_deref() {
        Some("-i") | Some("--icon") => println!("󰚰 {}", output.len() - 1),
        Some("-l") | Some("--long") => output.iter().for_each(|line| println!("{line}")),
        Some(_) | None => println!("{}", output.len() - 1),
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut output = Vec::new();
    output.push(format!(
        "{:<20} {:<15} {:<15}",
        "Port", "Version", "Available"
    ));

    match INSTALLED_PACKAGES.read() {
        Ok(packages) => {
            for (name, version) in packages.iter() {
                let repository =
                    find_ports_in_repositories(name).unwrap_or_else(|_| "N/A".to_string());
                let available_version =
                    extract_pkgfile_version(&format!("/usr/ports/{repository}/{name}"))
                        .unwrap_or_else(|| "N/A".to_string());

                let installed_version = version.as_deref().unwrap_or("unknown");

                let comparison = version_compare(available_version.as_str(), installed_version);

                if comparison == Ordering::Greater {
                    output.push(format!(
                        "{name:<20} {installed_version:<15} {available_version:<15}"
                    ));
                }
            }
        }
        Err(e) => {
            eprintln!("Could not acquire the read lock: {e}");
            return Err(Box::new(e));
        }
    }

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: scun [notify|print]");
        process::exit(1);
    }

    match args[1].as_str() {
        "notify" | "n" => notify_mode(output)?,
        "print" | "p" => print_mode(output, args.get(2).cloned())?,
        _ => {
            eprintln!("Invalid mode: {}. Use 'notify' or 'print'.", args[1]);
            process::exit(1);
        }
    }

    Ok(())
}
