fn terminal_prefix(
    args: &[OsString],
    trigger: &str,
    globals: &[(&str, Option<&str>, Option<u8>, bool)],
    allowed: &[&str],
    denied: &str,
    auxiliary: Command,
) -> Option<Invocation> {
    let mut index = 1;
    let mut permitted = true;
    loop {
        let token = args.get(index)?.as_encoded_bytes();
        if token == trigger.as_bytes() {
            break;
        }
        let (leading, attached) = match token {
            b"--" | b"--help" | b"-h" | b"--version" | b"-V" => return None,
            [b'-', b'-', name @ ..] => {
                let boundary = name
                    .iter()
                    .position(|byte| *byte == b'=')
                    .unwrap_or(name.len());
                (
                    globals.iter().find(|(_, long, _, _)| {
                        long.is_some_and(|long| long.as_bytes() == &name[..boundary])
                    }),
                    boundary != name.len(),
                )
            }
            [b'-', short, rest @ ..] => (
                globals
                    .iter()
                    .find(|(_, _, token, _)| token == &Some(*short)),
                !rest.is_empty(),
            ),
            _ => return None,
        };
        let (id, _, _, value) = leading?;
        if attached && !value {
            return None;
        }
        permitted &= allowed.contains(id);
        index += if *value && !attached { 2 } else { 1 };
    }
    // The leaf's successful parse is outside this terminal routing profile.
    // Leave direct selection to the existing auxiliary path and out-of-scope
    // separators to the primary parser; never dispatch from an auxiliary match.
    let tail = &args[index + 1..];
    if index == 1 || tail.iter().any(|argument| argument == "--") {
        return None;
    }
    let label = OsString::from(auxiliary.get_name());
    let error = auxiliary
        .try_get_matches_from(std::iter::once(label).chain(tail.iter().cloned()))
        .err()?;
    if permitted {
        Some(Invocation::Terminal(error))
    } else {
        Some(Invocation::Terminal(Error::raw(
            clap::error::ErrorKind::InvalidValue,
            format!("{denied}\n"),
        )))
    }
}
