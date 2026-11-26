use clap::{CommandFactory, Parser, Subcommand};
use std::{ffi::OsString, process::ExitCode};

#[derive(Parser)]
#[command(version, about = "Record observatory measurements")]
struct Telescope {
    /// Label all measurements with a site.
    #[arg(short, long, visible_alias = "station", global = true, default_value = "summit")]
    site: String,
    #[command(subcommand)]
    mode: Option<Mode>,
}

#[derive(Subcommand)]
enum Mode {
    #[command(visible_alias = "scan")]
    Capture(Capture),
    #[command(hide = true)]
    Calibrate,
}

/// Record a group of exposures.
#[derive(clap::Args)]
struct Capture {
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u64).range(1..=10))]
    count: u64,
    #[arg(long, conflicts_with = "silent")]
    verbose: bool,
    #[arg(long)]
    silent: bool,
    #[arg(long, requires = "silent")]
    export: bool,
    sensor: OsString,
}

fn main() -> ExitCode {
    match Telescope::command().bin_name("telescope").try_get_matches() {
        Ok(matches) => {
            if let Some(("capture", capture)) = matches.subcommand() {
                println!(
                    "urn:observatory:capture {} {} {}",
                    matches.get_one::<String>("site").unwrap(),
                    capture.get_one::<u64>("count").unwrap(),
                    capture.get_one::<OsString>("sensor").unwrap().to_string_lossy(),
                );
            } else {
                eprintln!("command operation unavailable");
                return ExitCode::from(125);
            }
        }
        Err(error) => {
            let invalid = error.use_stderr();
            let _ = error.print();
            if invalid {
                eprintln!("telescope: invalid arguments");
                return ExitCode::from(12);
            }
        }
    }
    ExitCode::SUCCESS
}
