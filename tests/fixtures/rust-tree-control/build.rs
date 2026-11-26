use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=inputs/delta.txt");
    let input = fs::read_to_string("inputs/delta.txt").expect("read delta input");
    let delta: i32 = input.trim().parse().expect("delta input is an integer");
    let destination = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    fs::write(
        destination.join("generated.rs"),
        format!("const GENERATED_DELTA: i32 = {delta};\n"),
    )
    .expect("write generated source");
}
