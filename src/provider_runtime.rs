//! Execution boundary for the import-free provider worlds in contracts/providers.

use wasmtime::component::types::{ComponentItem, Type};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store, StoreLimitsBuilder};

pub const MAX_COMPONENT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
const MEMORY_BYTES: usize = 32 * 1024 * 1024;
const FUEL: u64 = 50_000_000;

/// Invoke one provider in a fresh store. The caller validates document schemas
/// and any proposed effects before making them visible.
pub fn invoke(role: &str, component: &[u8], request: &[u8]) -> Result<Vec<u8>, String> {
    if !matches!(role, "generate" | "recover") {
        return Err(format!("unsupported provider role {role}"));
    }
    if component.len() > MAX_COMPONENT_BYTES {
        return Err("provider component exceeds size limit".into());
    }
    if request.len() > MAX_DOCUMENT_BYTES {
        return Err("provider request exceeds size limit".into());
    }
    execute(role, component, request).map_err(|error| format!("provider {role}: {error:#}"))
}

fn byte_list(ty: Type) -> bool {
    matches!(ty, Type::List(list) if matches!(list.ty(), Type::U8))
}

fn execute(role: &str, bytes: &[u8], request: &[u8]) -> wasmtime::Result<Vec<u8>> {
    let mut config = Config::new();
    config
        .wasm_component_model(true)
        // Store fuel starts at instantiation. Input size and disabled optimization
        // constrain compilation work, but do not impose a compiler CPU or RSS quota.
        .cranelift_opt_level(wasmtime::OptLevel::None)
        .consume_fuel(true)
        .memory_reservation(MEMORY_BYTES as u64)
        .memory_reservation_for_growth(0)
        .memory_init_cow(false);
    let engine = Engine::new(&config)?;
    let component = Component::from_binary(&engine, bytes)?;
    let world = component.component_type();
    if let Some((name, _)) = world.imports(&engine).next() {
        wasmtime::bail!("unexpected provider import {name}");
    }
    let mut exports = world.exports(&engine);
    let Some((name, export)) = exports.next() else {
        wasmtime::bail!("missing provider export {role}");
    };
    if name != role || exports.next().is_some() {
        wasmtime::bail!("expected exactly one provider export named {role}");
    }
    let ComponentItem::ComponentFunc(function) = export.ty else {
        wasmtime::bail!("provider export {role} must be a function");
    };
    let mut params = function.params();
    let mut results = function.results();
    if function.async_()
        || params.len() != 1
        || !params
            .next()
            .is_some_and(|(name, ty)| name == "request" && byte_list(ty))
        || results.len() != 1
        || !results.next().is_some_and(byte_list)
    {
        wasmtime::bail!("invalid signature for provider export {role}");
    }

    let limits = StoreLimitsBuilder::new()
        .instances(16)
        .memories(1)
        .memory_size(MEMORY_BYTES)
        .tables(4)
        .table_elements(16_384)
        .trap_on_grow_failure(true)
        .build();
    let mut store = Store::new(&engine, limits);
    store.limiter(|limits| limits);
    store.set_fuel(FUEL)?;
    let instance = Linker::new(&engine).instantiate(&mut store, &component)?;
    // Lifting Vec<u8> copies at most one bounded linear memory. It also
    // preserves the result across the guest's canonical post-return cleanup.
    let entry = instance.get_typed_func::<(&[u8],), (Vec<u8>,)>(&mut store, role)?;
    let (response,) = entry.call(&mut store, (request,))?;
    if response.len() > MAX_DOCUMENT_BYTES {
        wasmtime::bail!("provider response exceeds size limit");
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    // This test guest returns the input pointer/length via canonical ABI
    // lowering. Its allocator is sufficient for exactly one host call.
    const ECHO: &str = r#"(component
        (core module $module
            (memory (export "memory") 1)
            (func (export "alloc") (param i32 i32 i32 i32) (result i32) i32.const 32)
            (func (export "call") (param i32 i32) (result i32)
                i32.const 0 local.get 0 i32.store
                i32.const 4 local.get 1 i32.store
                i32.const 0))
        (core instance $i (instantiate $module))
        (func (export "generate") (param "request" (list u8)) (result (list u8))
            (canon lift (core func $i "call") (memory $i "memory")
                (realloc (func $i "alloc")))))"#;

    fn run(role: &str, wat: &str) -> Result<Vec<u8>, String> {
        invoke(role, &wat::parse_str(wat).unwrap(), br#"{"version":1}"#)
    }

    #[test]
    fn both_worlds_receive_exact_document_bytes() {
        for role in ["generate", "recover"] {
            assert_eq!(
                run(role, &ECHO.replace("generate", role)).unwrap(),
                br#"{"version":1}"#
            );
        }
    }

    #[test]
    fn checks_component_imports_and_export_shape_before_instantiation() {
        // An initial trap makes a validation-after-instantiation regression visible.
        let trapping = ECHO.replace(
            "(memory (export",
            "(func $start unreachable) (start $start) (memory (export",
        );
        for (component, expected) in [
            (
                trapping.replace("(component", "(component (import \"authority\" (func))"),
                "unexpected provider import",
            ),
            (
                trapping.replace("generate", "recover"),
                "expected exactly one provider export",
            ),
            (
                trapping.replace("param \"request\"", "param \"other\""),
                "invalid signature",
            ),
            (trapping.replace("list u8", "list s8"), "invalid signature"),
            (
                trapping.replace(
                    "(core instance $i",
                    "(export \"unused\" (core module $module)) (core instance $i",
                ),
                "expected exactly one provider export",
            ),
        ] {
            let error = run("generate", &component).unwrap_err();
            assert!(error.contains(expected), "{error}");
        }
    }

    #[test]
    fn limits_initial_memory_execution_and_document_sizes() {
        let large_memory = ECHO.replace("(export \"memory\") 1", "(export \"memory\") 600");
        let error = run("generate", &large_memory).unwrap_err();
        assert!(error.contains("memory"), "{error}");
        let spin = ECHO.replace(
            "i32.const 0 local.get 0",
            "(loop $spin br $spin) i32.const 0 local.get 0",
        );
        let error = run("generate", &spin).unwrap_err();
        assert!(error.contains("fuel"), "{error}");
        let large_result = ECHO
            .replace("(export \"memory\") 1", "(export \"memory\") 130")
            .replace("i32.const 4 local.get 1", "i32.const 4 i32.const 8388609");
        assert!(
            run("generate", &large_result)
                .unwrap_err()
                .contains("response exceeds")
        );
        assert!(
            invoke("recover", &[], &vec![0; MAX_DOCUMENT_BYTES + 1])
                .unwrap_err()
                .contains("request exceeds")
        );
        assert!(
            invoke("recover", &vec![0; MAX_COMPONENT_BYTES + 1], &[])
                .unwrap_err()
                .contains("component exceeds")
        );
    }
}
