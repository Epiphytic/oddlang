#![deny(warnings)]

mod different;
mod free;
mod projection;

#[derive(Debug, PartialEq, Eq)]
enum ValidationError {
    Empty,
    Syntax,
    Components,
    Scheme,
    Address,
    LocalOnly,
}

fn strings(entries: &[&str]) -> Vec<String> {
    entries.iter().map(|value| (*value).to_owned()).collect()
}

fn main() {
    use ValidationError::*;
    use projection::DownloadMirrors;

    for (raw, outcome) in [
        ("http://mirror.example", Err(Scheme)),
        ("http://mirror.example\n", Err(Syntax)),
        ("http://mirror.example@elsewhere.example", Err(Components)),
        ("http://mirror.example@elsewhere.example\u{a0}", Err(Syntax)),
        ("https://mirror.example/?", Err(Components)),
        ("https://mirror.example/path", Err(Components)),
        ("https://bad_label.example", Err(Address)),
        ("https://mirror..example", Err(Address)),
        ("https://local.example", Err(LocalOnly)),
        ("https://alt.local.example", Err(LocalOnly)),
        (
            "https://alt-local.example",
            Ok("https://alt-local.example/"),
        ),
        ("https://198.51.100.0", Ok("https://198.51.100.0/")),
        ("https://198.51.100.63", Ok("https://198.51.100.63/")),
        ("https://198.51.100.64", Err(Address)),
        ("https://198.51.100.127", Err(Address)),
        ("https://198.51.100.128", Ok("https://198.51.100.128/")),
        ("https://198.51.101.0", Err(Address)),
        ("https://[2001:db8::1]", Ok("https://[2001:db8::1]/")),
        ("https://[2001:db8:1::]", Err(Address)),
        ("https://[2001:db8:2::]", Ok("https://[2001:db8:2::]/")),
        ("https://[::ffff:198.51.100.0]", Err(Address)),
        ("https://127.1", Err(LocalOnly)),
        ("https://[::1]", Err(LocalOnly)),
    ] {
        let result = DownloadMirrors::checked(&strings(&[raw]), false);
        assert_eq!(
            result.as_ref().map(|entry| entry.urls()[0].as_str()),
            outcome.as_ref().copied(),
            "{raw}"
        );
    }
    assert_eq!(DownloadMirrors::checked(&[], true), Err(Empty));
    assert_eq!(
        DownloadMirrors::checked(&strings(&["ftp://elsewhere.example", "bad"]), false),
        Err(Scheme),
    );
    assert_eq!(
        DownloadMirrors::checked(&strings(&["bad", "ftp://elsewhere.example"]), false),
        Err(Syntax),
    );
    let result = DownloadMirrors::checked(
        &strings(&[
            "HTTPS://First.Example.:443",
            "https://second.example",
            "https://first.example/",
            "https://local.example.",
            "https://127.1",
        ]),
        true,
    )
    .unwrap();
    assert_eq!(
        result.urls(),
        strings(&[
            "https://first.example/",
            "https://second.example/",
            "https://local.example/",
            "https://127.0.0.1/",
        ])
    );
    assert_eq!(
        DownloadMirrors::checked(result.urls(), true).unwrap(),
        result
    );
    assert_eq!(free::normalize(result.urls(), true).unwrap(), result.urls());
    assert_eq!(
        free::normalize(&[], false),
        Err("urn:download-mirrors:empty")
    );

    assert!(different::normalize(&[], false).unwrap().is_empty());
    assert_eq!(
        different::normalize(&strings(&["http://user@elsewhere.example\u{a0}"]), false),
        Err("urn:download-mirrors:components"),
    );
    assert_eq!(
        different::normalize(
            &strings(&["https://first.example.", "https://first.example."]),
            false
        )
        .unwrap(),
        strings(&["https://first.example./", "https://first.example./"]),
    );

    for last in 0..=255 {
        let endpoint = format!("https://198.51.100.{last}");
        let parsed = DownloadMirrors::checked(&[endpoint], false);
        assert_eq!(parsed.is_ok(), !(64..128).contains(&last));
    }
}
