#[cfg(unix)]
mod unix;

fn main() -> provider_validation::Result {
    #[cfg(unix)]
    {
        unix::verify()
    }
    #[cfg(not(unix))]
    {
        Err("the file-profile qualification requires POSIX filesystem semantics".into())
    }
}
