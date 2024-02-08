mod cache;
mod ports;

use cache::*;
use ports::*;

use std::env;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

fn notify_mode(output: Vec<String>) -> Result<(), Box<dyn Error>> {
    libnotify::init("scun").map_err(|e| {
        eprintln!("Failed to initialize libnotify: {}", e);
        e
    })?;

    let notification_body = output.join("\n");
    let notification = libnotify::Notification::new("Port Updates", &*notification_body, None);
    notification.set_timeout(5000);
    notification.show().map_err(|e| {
        eprintln!("Failed to show notification: {}", e);
        e
    })?;

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

    match INSTALLED_PACKAGES.lock() {
        Ok(packages) => {
            packages.iter().for_each(|(name, version)| {
                let repository =
                    find_ports_in_repositories(name).unwrap_or_else(|_| "N/A".to_string());
                let available_version =
                    extract_pkgfile_version(&format!("/usr/ports/{}/{}", repository, name))
                        .unwrap_or_else(|| "N/A".to_string());

                if version.as_deref().unwrap_or("unknown") != available_version {
                    output.push(format!(
                        "{:<20} {:<15} {:<15}",
                        name,
                        version.as_deref().unwrap_or("unknown"),
                        available_version
                    ));
                }
            });
        }
        Err(e) => {
            eprintln!("Could not acquire the lock: {}", e);
            return Err(Box::new(e));
        }
    }

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: scun [notify|print]");
        process::exit(1);
    }

    match args[1].as_str() {
        "notify" | "n" => notify_mode(output),
        "print" | "p" => print_mode(output, args.get(2).cloned()),
        _ => {
            eprintln!("Invalid mode: {}. Use 'notify' or 'print'.", args[1]);
            process::exit(1);
        }
    }
}
