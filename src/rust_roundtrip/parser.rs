use super::{FileSyntax, Syntax};
use ra_ap_parser::LexedStr;
use ra_ap_syntax::{Edition, NodeOrToken, SourceFile, SyntaxKind, WalkEvent};

pub(super) fn parse(raw: &str, edition: &str) -> Result<FileSyntax, String> {
    let edition = parse_edition(edition)?;
    let bom = raw.starts_with('\u{feff}');
    let input = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    if input.len() > u32::MAX as usize {
        return Err("Rust source exceeds parser size limit".into());
    }
    let crlf: Vec<_> = input
        .match_indices("\r\n")
        .enumerate()
        .map(|(previous, (offset, _))| offset - previous)
        .collect();
    let normalized = input.replace("\r\n", "\n");
    let parsed = SourceFile::parse(&normalized, edition);
    if let Some(error) = parsed.errors().first() {
        return Err(format!("Rust syntax at {:?}: {error}", error.range()));
    }
    let root = parsed.syntax_node();
    if root.text().to_string() != normalized {
        return Err("Rust parser did not preserve normalized source".into());
    }

    let mut nodes = Vec::new();
    let mut parents = Vec::new();
    for event in root.preorder_with_tokens() {
        match event {
            WalkEvent::Enter(element) => {
                let index = nodes.len();
                if let Some(&parent) = parents.last() {
                    let Syntax::Branch { children, .. } = &mut nodes[parent] else {
                        unreachable!("syntax node parents are branches");
                    };
                    children.push(index);
                }
                match element {
                    NodeOrToken::Node(node) => {
                        if node.kind() == SyntaxKind::ERROR {
                            return Err(format!(
                                "Rust parser error node at {:?}",
                                node.text_range()
                            ));
                        }
                        nodes.push(Syntax::Branch {
                            kind: format!("{:?}", node.kind()),
                            children: Vec::new(),
                        });
                        parents.push(index);
                    }
                    NodeOrToken::Token(token) => {
                        add_token(&mut nodes, token.kind(), token.text(), edition)?;
                    }
                }
            }
            WalkEvent::Leave(NodeOrToken::Node(_)) => {
                parents.pop();
            }
            WalkEvent::Leave(NodeOrToken::Token(_)) => {}
        }
    }
    Ok(FileSyntax { bom, crlf, nodes })
}

pub(super) fn validate_leaf(
    spelling: &str,
    lexical_kind: &str,
    edition: &str,
) -> Result<(), String> {
    let edition = parse_edition(edition)?;
    let found = lex_one(edition, spelling, lexical_kind == "SHEBANG")?
        .ok_or("leaf spelling is not exactly one Rust token")?;
    if format!("{found:?}") != lexical_kind {
        return Err(format!(
            "expected lexical kind {lexical_kind}, found {found:?}"
        ));
    }
    Ok(())
}

fn parse_edition(edition: &str) -> Result<Edition, String> {
    edition
        .parse::<Edition>()
        .map_err(|error| error.to_string())
}

fn lex_one(edition: Edition, text: &str, shebang: bool) -> Result<Option<SyntaxKind>, String> {
    if shebang {
        let lexed = LexedStr::new(edition, text);
        return Ok((lexed.len() == 1
            && lexed.kind(0) == SyntaxKind::SHEBANG
            && !text.strip_suffix('\n').unwrap_or(text).contains('\n'))
        .then_some(SyntaxKind::SHEBANG));
    }
    let Some((kind, error)) = LexedStr::single_token(edition, text) else {
        return Ok(None);
    };
    if let Some(error) = error {
        return Err(format!("Rust token: {error}"));
    }
    if matches!(
        kind,
        SyntaxKind::ERROR | SyntaxKind::EOF | SyntaxKind::FRONTMATTER
    ) {
        return Err(format!("unsupported Rust token {kind:?}"));
    }
    Ok(Some(kind))
}

fn add_token(
    nodes: &mut Vec<Syntax>,
    kind: SyntaxKind,
    spelling: &str,
    edition: Edition,
) -> Result<(), String> {
    if let Some(lexical_kind) = lex_one(edition, spelling, kind == SyntaxKind::SHEBANG)? {
        nodes.push(Syntax::Leaf {
            kind: format!("{kind:?}"),
            lexical_kind: format!("{lexical_kind:?}"),
            spelling: spelling.to_owned(),
        });
        return Ok(());
    }
    // The CST groups joint punctuation such as `>>=` into a contextual
    // token. Represent its physical tokens as ordered leaves.
    let parts = LexedStr::new(edition, spelling);
    if !kind.is_punct()
        || parts.len() < 2
        || (0..parts.len()).any(|i| !parts.kind(i).is_punct() || parts.error(i).is_some())
    {
        return Err(format!(
            "cannot represent Rust token {kind:?} as lexical leaves"
        ));
    }
    let start = nodes.len() + 1;
    nodes.push(Syntax::Branch {
        kind: format!("token:{kind:?}"),
        children: (start..start + parts.len()).collect(),
    });
    for i in 0..parts.len() {
        let kind = format!("{:?}", parts.kind(i));
        nodes.push(Syntax::Leaf {
            lexical_kind: kind.clone(),
            kind,
            spelling: parts.text(i).to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse, validate_leaf};
    use crate::rust_roundtrip::Syntax;

    #[test]
    fn preserves_ordered_trivia_and_compound_tokens() {
        let source =
            "\u{feff}#!/usr/bin/env rust\r\nfn r#gen() { /* a /* b */ c */ let _ = 2 >> 1; }\r\n";
        let parsed = parse(source, "2024").unwrap();
        assert!(parsed.bom);
        assert_eq!(parsed.crlf, vec![19, 68]);
        let tokens: String = parsed
            .nodes
            .iter()
            .filter_map(|node| match node {
                Syntax::Leaf { spelling, .. } => Some(spelling.as_str()),
                Syntax::Branch { .. } => None,
            })
            .collect();
        assert_eq!(
            tokens,
            source.trim_start_matches('\u{feff}').replace("\r\n", "\n")
        );
        assert!(parsed.nodes.iter().any(|node| matches!(
            node, Syntax::Branch { kind, children } if kind.starts_with("token:") && children.len() == 2
        )));
    }

    #[test]
    fn rejects_invalid_or_mislabeled_leaves() {
        assert!(validate_leaf("fn f() {}", "IDENT", "2024").is_err());
        assert!(validate_leaf("name", "WHITESPACE", "2024").is_err());
        assert!(validate_leaf("/* unterminated", "COMMENT", "2024").is_err());
        assert!(validate_leaf("/* a /* b */ c */", "COMMENT", "2024").is_ok());
        assert!(validate_leaf("#!/usr/bin/env rust", "SHEBANG", "2024").is_ok());
    }
}
