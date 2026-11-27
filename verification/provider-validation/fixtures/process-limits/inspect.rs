use std::os::fd::AsRawFd;
fn limits() -> (libc::rlim_t, libc::rlim_t) {
    let mut value = libc::rlimit {rlim_cur: 0, rlim_max: 0};
    assert_eq!(unsafe {libc::getrlimit(libc::RLIMIT_NOFILE, &mut value)}, 0);
    (value.rlim_cur, value.rlim_max)
}
fn main() {
    // Establish the inherited soft limit in a dedicated child, then exec the probe.
    if let Some(requested) = std::env::args().nth(1) {
        use std::os::unix::process::CommandExt;
        let current = limits();
        let limit = libc::rlimit { rlim_cur: requested.parse().unwrap(), rlim_max: current.1 };
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) }, 0);
        panic!("{}", std::process::Command::new(std::env::current_exe().unwrap()).exec());
    }
    let file = std::fs::File::open("/dev/null").unwrap();
    let fd = file.as_raw_fd();
    let flags = unsafe {libc::fcntl(fd, libc::F_GETFD)};
    assert!(flags >= 0);
    let before = limits();
    let initialize: fn() -> water_monitor::Result<()> = water_monitor::initialize_budget;
    initialize().unwrap();
    let first = limits();
    initialize().unwrap();
    assert_eq!(limits(), first);
    assert_eq!(first.1, before.1);
    assert_eq!(unsafe {libc::fcntl(fd, libc::F_GETFD)}, flags);
    println!("{} {}", before.0, first.0);
    assert_eq!(water_monitor::BudgetError::Unavailable.to_string(), "budget unavailable");
}
