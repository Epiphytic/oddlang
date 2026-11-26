use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

static RECORDING: AtomicBool = AtomicBool::new(false);

pub fn begin() {
    RECORDING.store(true, Ordering::Relaxed);
    let _ = writeln!(
        std::io::stderr().lock(),
        "{}",
        serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "host": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "code": 7,
            "event": "startup"
        })
    );
}

pub fn observation(point: &'static str, reading: &'static str) {
    if RECORDING.load(Ordering::Relaxed) {
        let _ = writeln!(
            std::io::stderr().lock(),
            "{}",
            serde_json::json!({"point": point, "reading": reading, "finite": true})
        );
    }
}

pub fn mark(current: &mut &'static str, new: &'static str) {
    *current = new;
    observation(new, "initial\r\t");
}

pub fn end() {
    RECORDING.store(false, Ordering::Relaxed);
}
