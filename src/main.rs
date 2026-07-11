mod cache;
mod ports;

use libversion::version_compare2;
use ports::*;
use rayon::prelude::*;
use std::cmp::Ordering;
use std::env;
use std::error::Error;
use std::process;

const USAGE: &str = "\
Usage: scun [notify|n|print|p] [OPTION]

Modes:
  notify, n        Send a desktop notification with available updates
  print, p         Print the number of available updates

Print options:
  -i, --icon       Print the update count with an icon
  -l, --long       Print the update table
  -h, --help       Show this help text
";

#[derive(Debug, PartialEq, Eq)]
enum CliAction {
    Run(Command),
    Help,
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Notify,
    Print(PrintMode),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrintMode {
    Count,
    Icon,
    Long,
}

#[derive(Debug, PartialEq, Eq)]
enum CliError {
    MissingMode,
    InvalidMode(String),
    InvalidPrintOption(String),
    UnexpectedArgument {
        mode: &'static str,
        argument: String,
    },
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::MissingMode => write!(f, "Missing mode. Use 'notify' or 'print'."),
            CliError::InvalidMode(mode) => {
                write!(f, "Invalid mode: {mode}. Use 'notify' or 'print'.")
            }
            CliError::InvalidPrintOption(option) => {
                write!(
                    f,
                    "Invalid print option: {option}. Use '--icon' or '--long'."
                )
            }
            CliError::UnexpectedArgument { mode, argument } => {
                write!(f, "Unexpected argument for {mode}: {argument}.")
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct UpdateRow {
    name: String,
    installed_version: String,
    available_version: String,
}

fn is_help_arg(arg: &str) -> bool {
    matches!(arg, "-h" | "--help" | "help")
}

fn parse_args(args: &[String]) -> Result<CliAction, CliError> {
    let Some(mode) = args.first().map(String::as_str) else {
        return Err(CliError::MissingMode);
    };

    if is_help_arg(mode) {
        return Ok(CliAction::Help);
    }

    match mode {
        "notify" | "n" => {
            if let Some(argument) = args.get(1) {
                if is_help_arg(argument) {
                    Ok(CliAction::Help)
                } else {
                    Err(CliError::UnexpectedArgument {
                        mode: "notify",
                        argument: argument.clone(),
                    })
                }
            } else {
                Ok(CliAction::Run(Command::Notify))
            }
        }
        "print" | "p" => parse_print_args(&args[1..]),
        _ => Err(CliError::InvalidMode(mode.to_string())),
    }
}

fn parse_print_args(args: &[String]) -> Result<CliAction, CliError> {
    let Some(option) = args.first().map(String::as_str) else {
        return Ok(CliAction::Run(Command::Print(PrintMode::Count)));
    };

    if is_help_arg(option) {
        return Ok(CliAction::Help);
    }

    let mode = match option {
        "-i" | "--icon" => PrintMode::Icon,
        "-l" | "--long" => PrintMode::Long,
        _ => return Err(CliError::InvalidPrintOption(option.to_string())),
    };

    if let Some(argument) = args.get(1) {
        return Err(CliError::UnexpectedArgument {
            mode: "print",
            argument: argument.clone(),
        });
    }

    Ok(CliAction::Run(Command::Print(mode)))
}

fn configure_rayon_threads() {
    let Ok(value) = env::var("SCUN_THREADS") else {
        return;
    };

    match value.parse::<usize>() {
        Ok(count) if count > 0 => {
            if let Err(e) = rayon::ThreadPoolBuilder::new()
                .num_threads(count)
                .build_global()
            {
                eprintln!("Failed to set SCUN_THREADS={count}: {e}");
            }
        }
        _ => eprintln!("Invalid SCUN_THREADS value: {value}"),
    }
}

fn update_for_package((name, version): &PackageInfo) -> Option<UpdateRow> {
    let port_dir = find_ports_in_repositories(name)?;
    let available_version = extract_pkgfile_version(&port_dir)?;
    let installed_version = version.as_deref().unwrap_or("unknown");

    (version_compare2(&available_version, installed_version) == Ordering::Greater).then(|| {
        UpdateRow {
            name: name.to_string(),
            installed_version: installed_version.to_string(),
            available_version,
        }
    })
}

fn available_updates() -> Vec<UpdateRow> {
    let mut updates: Vec<(usize, UpdateRow)> = INSTALLED_PACKAGES
        .par_iter()
        .enumerate()
        .filter_map(|(idx, package)| update_for_package(package).map(|row| (idx, row)))
        .collect();

    updates.sort_unstable_by_key(|(idx, _)| *idx);
    updates.into_iter().map(|(_, row)| row).collect()
}

fn format_update_table(rows: &[UpdateRow]) -> Vec<String> {
    let header = ("Port", "Version", "Available");
    let (name_w, inst_w, avail_w) = rows.iter().fold(
        (header.0.len(), header.1.len(), header.2.len()),
        |(name_w, inst_w, avail_w), row| {
            (
                name_w.max(row.name.len()),
                inst_w.max(row.installed_version.len()),
                avail_w.max(row.available_version.len()),
            )
        },
    );

    let mut output = Vec::with_capacity(rows.len() + 2);
    output.push(format!(
        "{:<name_w$} {:<inst_w$} {:<avail_w$}",
        header.0,
        header.1,
        header.2,
        name_w = name_w,
        inst_w = inst_w,
        avail_w = avail_w
    ));
    output.push(format!(
        "{:-<name_w$} {:-<inst_w$} {:-<avail_w$}",
        "",
        "",
        "",
        name_w = name_w,
        inst_w = inst_w,
        avail_w = avail_w
    ));

    output.extend(rows.iter().map(|row| {
        format!(
            "{:<name_w$} {:<inst_w$} {:<avail_w$}",
            row.name,
            row.installed_version,
            row.available_version,
            name_w = name_w,
            inst_w = inst_w,
            avail_w = avail_w
        )
    }));

    output
}

fn notify_mode(updates: &[UpdateRow]) -> Result<(), Box<dyn Error>> {
    if libnotify::init("scun").is_err() {
        return Err("Failed to initialize libnotify".into());
    }

    let output = format_update_table(updates);
    let notification_body = output.join("\n");
    let notification =
        libnotify::Notification::new("Port Updates", Some(notification_body.as_str()), None);

    notification.set_timeout(5000);
    notification.show()?;

    libnotify::uninit();
    Ok(())
}

fn print_output(updates: &[UpdateRow], mode: PrintMode) -> Vec<String> {
    match mode {
        PrintMode::Count => vec![updates.len().to_string()],
        PrintMode::Icon => vec![format!("󰚰 {}", updates.len())],
        PrintMode::Long => format_update_table(updates),
    }
}

fn print_mode(updates: &[UpdateRow], mode: PrintMode) {
    for line in print_output(updates, mode) {
        println!("{line}");
    }
}

fn run(command: Command) -> Result<(), Box<dyn Error>> {
    configure_rayon_threads();
    let updates = available_updates();

    match command {
        Command::Notify => notify_mode(&updates)?,
        Command::Print(mode) => print_mode(&updates, mode),
    }

    Ok(())
}

fn cli_args() -> Vec<String> {
    env::args().skip(1).collect()
}

fn main() -> Result<(), Box<dyn Error>> {
    match parse_args(&cli_args()) {
        Ok(CliAction::Help) => println!("{USAGE}"),
        Ok(CliAction::Run(command)) => run(command)?,
        Err(e) => {
            eprintln!("{e}");
            eprintln!("{USAGE}");
            process::exit(2);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| arg.to_string()).collect()
    }

    fn update_rows() -> Vec<UpdateRow> {
        vec![
            UpdateRow {
                name: "foo".to_string(),
                installed_version: "1.0-1".to_string(),
                available_version: "1.1-1".to_string(),
            },
            UpdateRow {
                name: "longer-package".to_string(),
                installed_version: "2.0-1".to_string(),
                available_version: "2.0-2".to_string(),
            },
        ]
    }

    #[test]
    fn parse_args_defaults_print_to_count_mode() {
        assert_eq!(
            parse_args(&args(&["print"])),
            Ok(CliAction::Run(Command::Print(PrintMode::Count)))
        );
    }

    #[test]
    fn parse_args_accepts_print_submodes() {
        assert_eq!(
            parse_args(&args(&["p", "--icon"])),
            Ok(CliAction::Run(Command::Print(PrintMode::Icon)))
        );
        assert_eq!(
            parse_args(&args(&["print", "-l"])),
            Ok(CliAction::Run(Command::Print(PrintMode::Long)))
        );
    }

    #[test]
    fn parse_args_rejects_unknown_or_extra_arguments() {
        assert_eq!(parse_args(&args(&[])), Err(CliError::MissingMode));
        assert_eq!(
            parse_args(&args(&["bad"])),
            Err(CliError::InvalidMode("bad".to_string()))
        );
        assert_eq!(
            parse_args(&args(&["print", "--bad"])),
            Err(CliError::InvalidPrintOption("--bad".to_string()))
        );
        assert_eq!(
            parse_args(&args(&["notify", "--long"])),
            Err(CliError::UnexpectedArgument {
                mode: "notify",
                argument: "--long".to_string()
            })
        );
        assert_eq!(
            parse_args(&args(&["print", "--long", "extra"])),
            Err(CliError::UnexpectedArgument {
                mode: "print",
                argument: "extra".to_string()
            })
        );
    }

    #[test]
    fn parse_args_supports_help() {
        assert_eq!(parse_args(&args(&["--help"])), Ok(CliAction::Help));
        assert_eq!(parse_args(&args(&["print", "--help"])), Ok(CliAction::Help));
        assert_eq!(parse_args(&args(&["notify", "-h"])), Ok(CliAction::Help));
    }

    #[test]
    fn print_count_ignores_long_table_header_lines() {
        let rows = update_rows();

        assert_eq!(print_output(&rows, PrintMode::Count), vec!["2"]);
        assert_eq!(print_output(&rows, PrintMode::Icon), vec!["󰚰 2"]);
        assert_eq!(print_output(&rows, PrintMode::Long).len(), rows.len() + 2);
    }

    #[test]
    fn format_update_table_uses_dynamic_widths() {
        let output = format_update_table(&update_rows());

        assert_eq!(output[0], "Port           Version Available");
        assert_eq!(output[1], "-------------- ------- ---------");
        assert_eq!(output[2], "foo            1.0-1   1.1-1    ");
        assert_eq!(output[3], "longer-package 2.0-1   2.0-2    ");
    }
}
