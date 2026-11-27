use crate::*;
use regex::Regex;
use tree_sitter::{Node, Parser};

struct Rules {
    attr: Regex,
    path: Regex,
    include: Regex,
}

impl Rules {
    fn walk(
        &self,
        node: Node<'_>,
        source: &[u8],
        inline: &[String],
        out: &mut Syntax,
    ) -> Result<()> {
        let mut attrs = Vec::<String>::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "line_comment" | "block_comment" => continue,
                "attribute_item" => {
                    attrs.push(child.utf8_text(source)?.to_owned());
                    continue;
                }
                _ => {}
            }
            let cfg: Vec<_> = attrs
                .iter()
                .filter(|a| self.attr.is_match(a) || a.starts_with("#[tokio::test"))
                .cloned()
                .collect();
            let line = child.start_position().row + 1;
            let field = |name| {
                child
                    .child_by_field_name(name)
                    .context("missing Rust syntax field")?
                    .utf8_text(source)
                    .map_err(anyhow::Error::from)
            };
            let mut nested = inline.to_owned();
            if child.kind() == "function_item"
                && cfg
                    .iter()
                    .any(|a| a == "#[test]" || a.starts_with("#[tokio::test"))
            {
                out.tests
                    .push(json!({"name": field("name")?, "line":line, "attributes":cfg}));
            }
            if !cfg.is_empty() {
                out.attributes
                    .push((line, child.end_position().row + 1, cfg));
            }
            if child.kind() == "mod_item" {
                let name = field("name")?;
                if child.child_by_field_name("body").is_none() {
                    let path = attrs
                        .iter()
                        .find_map(|a| self.path.captures(a).map(|m| m[1].to_owned()));
                    out.edges.push(
                        json!({"kind":"mod","name":name,"path":path,"inline":inline,"line":line}),
                    );
                } else {
                    nested.push(name.to_owned());
                }
            }
            if child.kind() == "macro_invocation" && field("macro")? == "include" {
                let path = self
                    .include
                    .captures(child.utf8_text(source)?)
                    .map(|m| m[1].to_owned());
                out.edges
                    .push(json!({"kind":"include","path":path,"line":line}));
            }
            if child.named_child_count() > 0 {
                self.walk(child, source, &nested, out)?;
            }
            attrs.clear();
        }
        Ok(())
    }
}

#[derive(Default, serde::Serialize)]
pub struct Syntax {
    pub edges: Vec<Value>,
    pub tests: Vec<Value>,
    pub attributes: Vec<(usize, usize, Vec<String>)>,
}
impl Syntax {
    pub fn attrs(&self, line: usize) -> Vec<String> {
        self.attributes
            .iter()
            .filter(|(a, b, _)| (*a..=*b).contains(&line))
            .flat_map(|(_, _, attrs)| attrs.clone())
            .collect()
    }
}
pub type Workflows = BTreeMap<String, Vec<(String, Value)>>;
pub fn inspect(root: &Path, files: &Map) -> Result<(BTreeMap<String, Syntax>, Workflows)> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_rust::LANGUAGE.into())?;
    let rules = Rules {
        attr: Regex::new(r"^#\[\s*(cfg|cfg_attr|test|ignore)\b")?,
        path: Regex::new(r#"^#\[path\s*=\s*"([^"]+)"\]$"#)?,
        include: Regex::new(r#"^include!\s*\(\s*"([^"]+)"\s*\)$"#)?,
    };
    let mut syntax = BTreeMap::new();
    let mut workflows = Workflows::new();
    for path in files.keys() {
        if path.starts_with("crates/") && path.ends_with(".rs") {
            let src = fs::read(root.join(path))?;
            let tree = parser.parse(&src, None).context("could not parse Rust")?;
            let mut result = Syntax::default();
            rules.walk(tree.root_node(), &src, &[], &mut result)?;
            syntax.insert(path.clone(), result);
        }
        if Path::new(path).parent() == Some(Path::new(".github/workflows"))
            && path.ends_with(".yml")
        {
            let doc: serde_yaml::Value = serde_yaml::from_slice(&fs::read(root.join(path))?)?;
            let mut jobs = Vec::new();
            for (id, job) in doc["jobs"].as_mapping().context("expected workflow jobs")? {
                jobs.push((
                    id.as_str().context("expected job name")?.to_string(),
                    serde_json::to_value(job)?,
                ));
            }
            workflows.insert(path.to_string(), jobs);
        }
    }
    Ok((syntax, workflows))
}
