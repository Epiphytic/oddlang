
use clap::{CommandFactory, Parser, Subcommand, error::ErrorKind};
use std::{ffi::OsString, process::ExitCode};

#[derive(Parser)]
#[command(version, about = "Primary readings")]
struct Primary {
    #[arg(short = 'p', long, global = true)] place: Option<String>,
    #[arg(short = 'a', long, global = true)] agent: Option<String>,
    #[arg(long, global = true)] brief: bool,
    #[command(subcommand)] command: Option<Mode>,
}
#[derive(Subcommand)]
enum Mode { Survey }
#[derive(Parser)]
#[command(name = "meter survey", about = "Survey readings")]
struct Survey { label: String }

fn accept(field: &str) -> bool { matches!(field, "place" | "brief") }
fn invalid(reason: &'static str) -> clap::Error {
    clap::Error::raw(ErrorKind::InvalidValue, format!("meter: {reason}\n"))
}
fn check(before: &[&str]) -> Result<(), clap::Error> {
    if before.iter().any(|field| !accept(field)) {
        return Err(invalid("option rejected"));
    }
    Ok(())
}
fn main() -> ExitCode {
    let args = std::env::args_os().collect::<Vec<_>>();
    let mut prefix = Vec::new();
    let mut index = 1;
    while let Some(argument) = args.get(index).and_then(|argument| argument.to_str()) {
        let (id, count) = match argument {
            "--brief" => ("brief", 1),
            "-p" | "--place" => ("place", 2),
            "-a" | "--agent" => ("agent", 2),
            value if value.starts_with("--place=") || (value.starts_with("-p") && value.len() > 2) => ("place", 1),
            value if value.starts_with("--agent=") || (value.starts_with("-a") && value.len() > 2) => ("agent", 1),
            _ => break,
        };
        prefix.push(id);
        index += count;
    }
    let root = Primary::command().bin_name("meter");
    let parsed = if args.get(index).is_some_and(|arg| arg == "survey") {
        check(&prefix).and_then(|_| Survey::command().try_get_matches_from(
            std::iter::once(OsString::from("meter survey")).chain(args[index+1..].iter().cloned())))
    } else {
        root.try_get_matches_from(args)
    };
    match parsed {
        Ok(_) => { eprintln!("command operation unavailable"); ExitCode::from(125) }
        Err(error) => {
            let invalid = error.use_stderr();
            error.print().unwrap();
            if invalid { eprintln!("meter: invalid arguments"); ExitCode::from(18) }
            else { ExitCode::SUCCESS }
        }
    }
}
