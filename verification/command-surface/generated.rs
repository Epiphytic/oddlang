mod cli;
use std::{ffi::OsString, process::ExitCode};

// Exercise the default entry point independently of the observing handler.
const _: fn() -> ExitCode = cli::run_cli;

fn main() -> ExitCode {
    cli::run_cli_with(|operation, matches| {
        let capture = matches.subcommand_matches("capture").unwrap();
        println!(
            "{operation} {} {} {}",
            matches.get_one::<String>("site").unwrap(),
            capture.get_one::<u64>("count").unwrap(),
            capture.get_one::<OsString>("sensor").unwrap().to_string_lossy(),
        );
        ExitCode::SUCCESS
    })
}
