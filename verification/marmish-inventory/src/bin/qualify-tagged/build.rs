use crate::{
    PIN,
    corpus::directory,
    native::{checked, command, sum},
    protocol::Adapters,
    sandbox::{Denied, Tools, environment, policy, probes},
};
use anyhow::{Context, Result, ensure};
use marmish_inventory::read_json;
use serde_json::{Value as V, json};
use std::{fs, path::Path};
pub fn project(
    stage: &Path,
    oddlang: &Path,
    data: &[u8],
    approved: bool,
    denied: &Denied,
    tools: &Tools,
) -> Result<V> {
    fs::create_dir(stage)?;
    let builder = stage.join("oddlang");
    fs::copy(oddlang, &builder)?;
    let graph = stage.join("data.ttl");
    fs::write(&graph, data)?;
    let prefix = tools.boundary(stage, &[], &[])?;
    let labels = probes(&prefix, stage, denied)?;
    let launch = |args: &[String]| {
        let mut c = command(&prefix)?;
        environment(&mut c, stage, None).arg(&builder).args(args);
        checked(&mut c, 60)
    };
    let pin = launch(&[
        "tagged-wire".into(),
        "digest".into(),
        graph.display().to_string(),
    ])?
    .trim()
    .to_owned();
    ensure!(
        pin.len() == 64
            && pin.bytes().all(|b| matches!(b,b'0'..=b'9'|b'a'..=b'f'))
            && (!approved || pin == PIN),
        "release graph digest mismatch: {pin}"
    );
    let mut locks = json!({});
    let mut outputs = json!({});
    let mut commands = json!({});
    for (target, name) in [("rust", "wire.rs"), ("typescript", "wire.mts")] {
        let dir = stage.join(target);
        let args = [
            "build",
            "tagged-wire",
            target,
            graph.to_str().context("graph path")?,
            dir.to_str().context("stage")?,
            "--graph-sha256",
            &pin,
        ]
        .map(str::to_owned);
        launch(&args)?;
        let lock = dir.join("projection.lock");
        ensure!(
            read_json(&lock)?
                == json!({"vocabulary":"oddlang-tagged-wire-v1","graphSha256":pin,"target":target,"source":name}),
            "projection lock mismatch"
        );
        locks[target] = json!(sum(&lock)?);
        outputs[target] = json!(sum(&dir.join(name))?);
        commands[target] = json!(args.map(|s| s.replace(stage.to_str().unwrap(), "$STAGE")));
    }
    Ok(
        json!({"graphSha256":pin,"graphFileSha256":sum(&graph)?,"generatorSha256":sum(&builder)?,"generatedSourceSha256":outputs,"projectionLockSha256":locks,"deniedReadProbes":labels,"commandArguments":commands,"sandboxProfileSha256":policy(&prefix,stage)?}),
    )
}
pub fn compile(stage: &Path, tools: &Tools, denied: &Denied, evidence: &mut V) -> Result<Adapters> {
    let rust = stage.join("rust");
    let js = stage.join("typescript");
    let check = || -> Result<_> {
        Ok(json!({"rust":sum(&rust.join("wire.rs"))?,"typescript":sum(&js.join("wire.mts"))?}))
    };
    ensure!(
        check()? == evidence["generatedSourceSha256"],
        "generated input changed"
    );
    fs::copy(
        directory().join("tagged_adapter.rs"),
        rust.join("adapter.rs"),
    )?;
    fs::copy(
        directory().join("tagged_adapter.mjs"),
        js.join("adapter.mjs"),
    )?;
    let reads: Vec<_> = std::iter::once(
        tools
            .rustc
            .parent()
            .and_then(Path::parent)
            .context("toolchain root")?,
    )
    .chain(tools.developer.as_deref())
    .collect();
    let native = tools.boundary(stage, &reads, &[])?;
    let rust_labels = probes(&native, stage, denied)?;
    let exe = rust.join("adapter");
    let mut c = command(&native)?;
    environment(&mut c, stage, tools.sdk.as_deref())
        .arg(&tools.rustc)
        .args(["--edition=2024", "-C"])
        .arg(format!("linker={}", tools.clang.display()))
        .arg(rust.join("adapter.rs"))
        .arg("-o")
        .arg(&exe);
    checked(&mut c, 60)?;
    let node = tools.boundary(stage, &[], &[&tools.node])?;
    let node_labels = probes(&node, stage, denied)?;
    ensure!(
        check()? == evidence["generatedSourceSha256"],
        "generated source changed"
    );
    evidence["adapterBoundaries"] = json!({"rustCompile":{"deniedReadProbes":rust_labels,"sandboxProfileSha256":policy(&native,stage)?},"nodeRuntime":{"deniedReadProbes":node_labels,"sandboxProfileSha256":policy(&node,stage)?}});
    let mut r = tools.boundary(stage, &[], &[])?;
    r.push(exe.display().to_string());
    let mut n = node;
    n.extend([
        tools.node.display().to_string(),
        "--no-warnings".into(),
        "--experimental-strip-types".into(),
        js.join("adapter.mjs").display().to_string(),
    ]);
    Ok([("rust".into(), r), ("typescript".into(), n)]
        .into_iter()
        .collect())
}
