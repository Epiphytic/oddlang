//! Independent example implementation covering flat records of primitive unsigned integers and booleans.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

fn artifact(name: &str, bytes: &[u8]) -> Value {
    json!({"path":name,"base64":STANDARD.encode(bytes)})
}

fn primitive(value: &str) -> &str {
    match value {
        "bool" | "u32" | "u64" => value,
        _ => panic!("unsupported example field type"),
    }
}

#[cfg(not(feature = "recover"))]
fn run(request: Value) -> Value {
    let input = &request["input"];
    if request["capability"] == "example.field-list" {
        let fields = input["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| field.as_str().unwrap())
            .collect::<Vec<_>>();
        return output(
            vec![artifact("fields.txt", fields.join("\n").as_bytes())],
            vec![],
            vec![],
            vec![],
        );
    }
    if request["capability"] == "example.library-binding" {
        let path = request["configuration"]["rustType"].as_str().unwrap();
        let binding = json!({"id":input["id"],"rustType":path});
        return output(
            vec![artifact(
                "binding.json",
                &serde_json::to_vec(&binding).unwrap(),
            )],
            vec![],
            vec![],
            vec![],
        );
    }
    let graph = &input["graph"];
    let overlay = &input["overlay"];
    assert_eq!(overlay["flattenModule"], true);
    let ids = &overlay["identifiers"];
    let mut code = String::new();
    let mut exports = Vec::new();
    for record in graph["modules"][0]["records"].as_array().unwrap() {
        assert!(record["traits"].as_array().unwrap().is_empty());
        let name = ids[record["id"].as_str().unwrap()].as_str().unwrap();
        exports.push(name.to_owned());
        code.push_str(&format!("pub struct {name} {{\n"));
        for field in record["fields"].as_array().unwrap() {
            let ty = primitive(field["type"].as_str().unwrap());
            let field = field["wire"].as_str().unwrap();
            code.push_str(&format!("    pub {field}: {ty},\n"));
        }
        code.push_str("}\n");
    }
    output(
        vec![artifact("records.rs", code.as_bytes())],
        exports,
        vec![],
        vec![],
    )
}

#[cfg(feature = "recover")]
fn run(request: Value) -> Value {
    let input = &request["input"];
    let source = &input["sources"][0];
    let bytes = STANDARD.decode(source["base64"].as_str().unwrap()).unwrap();
    let file = syn::parse_file(std::str::from_utf8(&bytes).unwrap()).unwrap();
    let selection = &input["selection"];
    let module = selection["id"].as_str().unwrap();
    let mut ids = serde_json::Map::new();
    ids.insert(module.into(), json!("root"));
    let mut records = Vec::new();
    assert!(file.attrs.is_empty());
    for item in file.items {
        let syn::Item::Struct(item) = item else {
            panic!("example expects public structs");
        };
        assert!(
            item.attrs.is_empty()
                && matches!(item.vis, syn::Visibility::Public(_))
                && item.generics.params.is_empty()
                && item.generics.where_clause.is_none()
        );
        let name = item.ident.to_string();
        let id = selection["records"][&name].as_str().unwrap();
        ids.insert(id.into(), json!(name));
        let mut fields = Vec::new();
        let syn::Fields::Named(named) = item.fields else {
            panic!("expected named fields")
        };
        for field in named.named {
            assert!(field.attrs.is_empty() && matches!(field.vis, syn::Visibility::Public(_)));
            let wire = field.ident.unwrap().to_string();
            let syn::Type::Path(ty) = field.ty else {
                panic!("unsupported field type");
            };
            assert!(ty.qself.is_none());
            let ty = ty.path.get_ident().unwrap().to_string();
            fields
                .push(json!({"id":format!("{id}/field/{wire}"),"wire":wire,"type":primitive(&ty)}));
        }
        records.push(json!({"id":id,"traits":[],"fields":fields}));
    }
    let graph = json!({"format":"oddlang-public-records-v1","modules":[{"id":module,"records":records,"aliases":[]}]});
    let overlay =
        json!({"format":"oddlang-public-records-rust-v1","flattenModule":true,"identifiers":ids});
    output(
        vec![
            artifact("graph.json", &serde_json::to_vec(&graph).unwrap()),
            artifact("rust.json", &serde_json::to_vec(&overlay).unwrap()),
        ],
        vec![],
        vec![source["path"].as_str().unwrap()],
        vec!["Rust derive and type bindings require downstream native compilation"],
    )
}

fn output(
    artifacts: Vec<Value>,
    exports: Vec<String>,
    evidence: Vec<&str>,
    gaps: Vec<&str>,
) -> Value {
    json!({"format":"oddlang-provider-output-v1","artifacts":artifacts,"exports":exports,"dependencies":[],"evidence":evidence,"gaps":gaps})
}

// Canonical ABI for the one list<u8> parameter and list<u8> result in provider.wit.
// The host creates one fresh instance per call and drops its memory after lifting the result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cabi_realloc(
    old: *mut u8,
    old_len: usize,
    align: usize,
    len: usize,
) -> *mut u8 {
    use std::alloc::{Layout, alloc, realloc};
    if old_len == 0 {
        if len == 0 {
            return align as *mut u8;
        }
        unsafe { alloc(Layout::from_size_align(len, align).unwrap()) }
    } else {
        unsafe { realloc(old, Layout::from_size_align(old_len, align).unwrap(), len) }
    }
}

#[cfg_attr(feature = "recover", unsafe(export_name = "recover"))]
#[cfg_attr(not(feature = "recover"), unsafe(export_name = "generate"))]
pub unsafe extern "C" fn call(ptr: *const u8, len: usize) -> *const [usize; 2] {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let result = run(serde_json::from_slice(bytes).unwrap());
    let result = serde_json::to_vec(&result).unwrap().into_boxed_slice();
    let result = Box::leak(result);
    Box::into_raw(Box::new([result.as_ptr() as usize, result.len()]))
}
