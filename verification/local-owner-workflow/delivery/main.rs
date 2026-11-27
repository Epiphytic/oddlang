mod generated;
fn main() -> std::process::ExitCode {
    let command = clap::Command::new("field-meter").subcommand(
        clap::Command::new("read").arg(clap::Arg::new("port").long("port")
            .value_parser(clap::value_parser!(std::path::PathBuf))));
    let mut args = command.get_matches();
    generated::measure(args.remove_subcommand().unwrap().1)
}
