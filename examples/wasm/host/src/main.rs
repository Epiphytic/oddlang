use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store};

fn main() -> wasmtime::Result<()> {
    let path = std::env::args_os().nth(1).expect("expected component path");
    let engine = Engine::default();
    let component = Component::from_file(&engine, path)?;
    let imports: Vec<_> = component
        .component_type()
        .imports(&engine)
        .map(|(name, _)| name.to_owned())
        .collect();
    assert!(imports.is_empty(), "component imports {imports:?}");
    let mut store = Store::new(&engine, ());
    let linker = Linker::<()>::new(&engine);
    let instance = linker.instantiate(&mut store, &component)?;
    let echo = instance.get_typed_func::<(i64,), (i64,)>(&mut store, "echo")?;
    println!("resolved imports = 0, linker definitions = 0");
    for input in [i64::MIN, -1, 0, 9_007_199_254_740_993, i64::MAX] {
        let (output,) = echo.call(&mut store, (input,))?;
        assert_eq!(input, output);
        println!("echo({input}) = {output}");
    }
    Ok(())
}
