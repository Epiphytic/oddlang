fn terminal_prefix(
    args: &[OsString],
    trigger: &str,
    globals: &[(&str, Option<&str>, Option<u8>, bool, bool)],
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
                    globals.iter().find(|(_, long, _, _, _)| {
                        long.is_some_and(|long| long.as_bytes() == &name[..boundary])
                    }),
                    boundary != name.len(),
                )
            }
            [b'-', short, rest @ ..] => (
                globals
                    .iter()
                    .find(|(_, _, token, _, _)| token == &Some(*short)),
                !rest.is_empty(),
            ),
            _ => return None,
        };
        let (id, _, _, value, hyphen_values) = leading?;
        if attached && !value {
            return None;
        }
        permitted &= allowed.contains(id);
        if *value && !attached {
            if !hyphen_values && args.get(index + 1)?.as_encoded_bytes().starts_with(b"-") {
                return None;
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    if index == 1 {
        return None;
    }
    if !permitted {
        return Some(Invocation::Terminal(Error::raw(
            clap::error::ErrorKind::InvalidValue,
            format!("{denied}\n"),
        )));
    }
    // A permitted prefix only contributes a terminal parser response.
    // Neither a separator nor a successful auxiliary parse routes here.
    let tail = &args[index + 1..];
    if tail.iter().any(|argument| argument == "--") {
        return None;
    }
    let label = OsString::from(auxiliary.get_name());
    auxiliary
        .try_get_matches_from(std::iter::once(label).chain(tail.iter().cloned()))
        .err()
        .map(Invocation::Terminal)
}
