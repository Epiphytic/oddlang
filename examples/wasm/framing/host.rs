use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store};

mod batch;

wasmtime::component::bindgen!({
    path: "../framing/host.wit",
    world: "framing",
});

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("expected a component path")?;
    let engine = Engine::default();
    let component = Component::from_file(&engine, path)?;
    if component.component_type().imports(&engine).next().is_some() {
        return Err("framing component requires imports".into());
    }
    let mut store = Store::new(&engine, ());
    let linker = Linker::new(&engine);
    let framing = Framing::instantiate(&mut store, &component, &linker)?;
    let codec = framing.oddlang_framing_codec();
    batch::run(|encode, input| {
        let result = if encode {
            codec.call_encode(&mut store, input)?
        } else {
            codec.call_decode(&mut store, input)?
        };
        use exports::oddlang::framing::codec::FrameError;
        Ok(result.map_err(|e| {
            match e {
                FrameError::SizeLimit => "SizeLimit",
                FrameError::InvalidJson => "InvalidJson",
                FrameError::InvalidVersion => "InvalidVersion",
                FrameError::InvalidEncoding => "InvalidEncoding",
            }
            .to_owned()
        }))
    })
}
