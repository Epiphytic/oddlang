use oddlang::model::load_model;
use std::{env, process::ExitCode};
fn run(args: &[String]) -> Result<(), String> {
    match args {
        [cmd, dir] if cmd == "check" => {
            load_model(dir)?;
            Ok(())
        }
        _ => Err("usage: oddlang check PROJECT".into()),
    }
}
fn main() -> ExitCode {
    match run(&env::args().skip(1).collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
