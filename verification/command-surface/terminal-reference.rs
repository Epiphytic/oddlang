
use clap::{CommandFactory, Parser};
use std::{ffi::OsString, process::ExitCode};

#[derive(Parser)]
#[command(name = "telescope survey", about = "Inspect sample labels")]
struct Survey {
    #[arg(long, default_value_t = 4, value_parser = clap::value_parser!(u64).range(1..=9))]
    limit: u64,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 1.., required = true)]
    labels: Vec<OsString>,
}

fn main() -> ExitCode {
    let args = std::env::args_os().skip(2);
    let parsed = Survey::command().try_get_matches_from(
        std::iter::once(OsString::from("telescope survey")).chain(args),
    );
    match parsed {
        Ok(_) => {
            eprintln!("command operation unavailable");
            ExitCode::from(125)
        }
        Err(error) => {
            let invalid = error.use_stderr();
            error.print().unwrap();
            if invalid {
                eprintln!("telescope: invalid arguments");
                ExitCode::from(12)
            } else {
                ExitCode::SUCCESS
            }
        }
    }
}
