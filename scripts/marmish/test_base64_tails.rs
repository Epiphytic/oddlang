// Compare every possible padded final quartet with the unique RFC 4648 spelling.
use base64::{engine::general_purpose::STANDARD, Engine};

fn check(frame: [u8; 4], canonical: bool) {
    match STANDARD.decode(frame) {
        Ok(bytes) => {
            assert!(canonical);
            assert_eq!(STANDARD.encode(bytes).as_bytes(), &frame);
        }
        Err(_) => assert!(!canonical),
    }
}

fn main() {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for &a in alphabet {
        for (j, &b) in alphabet.iter().enumerate() {
            check([a, b, b'=', b'='], j % 16 == 0);
            for (k, &c) in alphabet.iter().enumerate() {
                check([a, b, c, b'='], k % 4 == 0);
            }
        }
    }
}
