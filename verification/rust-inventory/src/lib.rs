//! Cargo verbose trace decoding shared by both Rust observers.
// Cargo wraps the shell-formatted command in backticks. A quoted argument may cross
// a physical log line. Match Python's POSIX shlex.split with comments disabled.
fn words(input: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut active = false;
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if quote == Some('\'') {
            if c == '\'' {
                quote = None;
            } else {
                word.push(c);
            }
        } else if c == '\\' {
            let escaped = chars.next()?;
            if quote == Some('"') && escaped != '\\' && escaped != '"' {
                word.push('\\');
            }
            word.push(escaped);
            active = true;
        } else if quote == Some('"') {
            if c == '"' {
                quote = None;
            } else {
                word.push(c);
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
            active = true;
        } else if matches!(c, ' ' | '\n' | '\t' | '\r') {
            if active {
                out.push(std::mem::take(&mut word));
                active = false;
            }
        } else {
            word.push(c);
            active = true;
        }
    }
    if quote.is_some() {
        return None;
    }
    if active {
        out.push(word);
    }
    Some(out)
}

pub fn commands(log: &str) -> Result<Vec<Vec<String>>, Box<dyn std::error::Error>> {
    let mut pending: Option<String> = None;
    let mut result = Vec::new();
    for line in log.lines() {
        if let Some(text) = &mut pending {
            text.push('\n');
            text.push_str(line);
        } else if line.starts_with(char::is_whitespace) {
            pending = line
                .trim_start()
                .strip_prefix("Running `")
                .map(str::to_owned);
        }
        if let Some(parsed) = pending
            .as_deref()
            .and_then(|s| s.strip_suffix('`'))
            .and_then(words)
        {
            result.push(parsed);
            pending = None;
        }
    }
    if pending.is_some() {
        return Err("incomplete verbose Cargo command".into());
    }
    Ok(result)
}

pub fn flag<'a>(args: &'a [String], name: &str) -> Vec<&'a str> {
    args.windows(2)
        .filter(|p| p[0] == name)
        .map(|p| p[1].as_str())
        .collect()
}
