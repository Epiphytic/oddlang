use super::*;
use serde_json::json;
// serde_json::Value sorts object keys. Read the compiler's actual output order.
#[derive(Deserialize)]
struct OrderedReceipt {
    #[serde(deserialize_with = "ordered_names")]
    cases: Vec<String>,
}
fn ordered_names<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Vec<String>, D::Error> {
    use serde::de::{IgnoredAny, MapAccess, Visitor};
    struct Names;
    impl<'de> Visitor<'de> for Names {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a frame case object")
        }
        fn visit_map<A: MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut names = Vec::new();
            while let Some((name, _)) = map.next_entry::<String, IgnoredAny>()? {
                names.push(name);
            }
            Ok(names)
        }
    }
    d.deserialize_map(Names)
}
pub(super) fn verify(s: &Suite, cases: &[Case], base: &str) -> Result {
    for case in cases {
        let source = case.revision(base)?;
        fs::write(&s.source, &source)?;
        let receipt = s.observe(&s.work.join("compiler.json"))?;
        assert_eq!(receipt["format"], "oddlang-rust-frame-fragments-v1");
        let Some(expected) = &case.expected else {
            unresolved(&receipt, None);
            assert!(receipt.get("cases").is_none());
            continue;
        };
        resolved(&receipt);
        let ordered: OrderedReceipt =
            serde_json::from_slice(&fs::read(s.work.join("compiler.json"))?)?;
        assert_eq!(ordered.cases, ["Copy", "Head"]);
        let output = s.work.join(&case.name);
        fs::create_dir(&output)?;
        let mut code = String::from(
            "#[derive(Debug)] pub enum Fault { Invalid, Limit }\n#[derive(Clone,Copy,PartialEq,Eq)] pub enum Level { Low, High }\n",
        );
        for variant in ["Head", "Copy"] {
            let mut boundary = receipt["cases"][variant].clone();
            boundary
                .as_object_mut()
                .unwrap()
                .remove("slots")
                .ok_or("missing fragment slots")?;
            let order = if variant == "Head" {
                json!(["urn:parts:version"])
            } else {
                json!([])
            };
            s.select(json!({"format":"oddlang-frame-fragment-selection-v1","method":"crate::Envelope::encode",
                "variant":variant,"boundary":boundary,"validationOrder":order}))?;
            let lower = variant.to_lowercase();
            let overlay = s.fixtures.join(format!("{lower}.rust.json"));
            let recovered = output.join(format!("{variant}-recovered"));
            s.recover(
                "frame-fragment",
                &s.fixtures.join(format!("{lower}.template.json")),
                &overlay,
                &recovered,
                None,
            )?;
            assert_eq!(
                read(&recovered.join("dependencies.json"))?["remainingIndependentObligations"],
                json!(["validator-semantics", "validation-order", "decoder"])
            );
            let generated = s.build(
                "binary-record",
                &recovered.join("graph.json"),
                &overlay,
                &output.join(variant),
            )?;
            code.push_str(&format!(
                "mod module_{lower} {{ {generated} }}\nuse module_{lower}::{variant};\n"
            ));
        }
        code.push_str(&s.build(
            "binary-frame",
            &s.fixtures.join("frame.json"),
            &s.fixtures.join("frame-rust.json"),
            &output.join("frame"),
        )?);
        s.native(&source, expected)?;
        s.native(&code, expected)?;
    }
    Ok(())
}
