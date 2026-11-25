mod echo;
mod ledger;

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, env, fs};
use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store};

#[derive(Deserialize)]
struct Request {
    interfaces: Vec<String>,
    imports: Vec<String>,
    exports: Vec<String>,
    #[serde(rename = "echoLayouts")]
    echo_layouts: BTreeMap<String, Vec<String>>,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    #[serde(rename = "caseId")]
    id: String,
    imports: Vec<Value>,
    calls: Vec<Call>,
}
#[derive(Deserialize)]
struct Call {
    function: String,
    arguments: Vec<Value>,
}
struct State {
    script: Vec<Value>,
    transcript: Vec<Value>,
}

impl State {
    fn imported(&mut self, name: &str) -> Result<Value> {
        // Repeated calls stay observable. The last value for that import
        // remains available so a duplicate does not itself induce a trap.
        let count = self
            .transcript
            .iter()
            .filter(|entry| entry["function"] == name)
            .count();
        let mut scripted = self.script.iter().filter(|entry| entry["function"] == name);
        let last = scripted.clone().next_back();
        let entry = scripted.nth(count).or(last).context("unscripted import")?;
        ensure!(
            entry["arguments"] == json!([]),
            "unexpected scripted arguments"
        );
        let value = entry["returns"].clone();
        self.transcript
            .push(json!({"function":name,"arguments":[],"returns":value}));
        Ok(value)
    }
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let component_path = args.next().context("missing component")?;
    let request: Request =
        serde_json::from_slice(&fs::read(args.next().context("missing cases")?)?)?;
    let engine = Engine::default();
    let component =
        Component::from_file(&engine, component_path).map_err(|e| e.context("load-failure"))?;
    let mut observations = Vec::new();
    for case in request.cases {
        let mut linker = Linker::<State>::new(&engine);
        for interface in &request.interfaces {
            // A use of a nominal type creates a type-only interface import.
            linker.instance(interface)?;
        }
        let mut store = Store::new(
            &engine,
            State {
                script: case.imports,
                transcript: Vec::new(),
            },
        );
        for name in &request.imports {
            ensure!(
                store.data().script.iter().any(|v| v["function"] == *name),
                "unprovided import {name} in {}",
                case.id
            );
            match name.as_str() {
                "initial-state" => {
                    linker.root().func_wrap(name, |mut cx, ()| {
                        let v = cx
                            .data_mut()
                            .imported("initial-state")
                            .map_err(wasmtime::Error::from_anyhow)?;
                        Ok((ledger::accounts(&v).map_err(wasmtime::Error::from_anyhow)?,))
                    })?;
                }
                "seed" => {
                    linker.root().func_wrap(name, |mut cx, ()| {
                        let v = cx
                            .data_mut()
                            .imported("seed")
                            .map_err(wasmtime::Error::from_anyhow)?;
                        Ok((decimal(&v).map_err(wasmtime::Error::from_anyhow)?,))
                    })?;
                }
                _ => bail!("unsupported host import {name}"),
            }
        }
        let instance = linker
            .instantiate(&mut store, &component)
            .map_err(|e| e.context("link-failure"))?;
        let mut bindings = BTreeMap::new();
        for name in &request.exports {
            let binding = match name.as_str() {
                "echo" | "mirror" => Binding::Echo(
                    echo::Binding::bind(
                        &mut store,
                        &instance,
                        name,
                        request
                            .echo_layouts
                            .get(name)
                            .context("unsupported echo parameter shape")?,
                    )
                    .context("type-failure")?,
                ),
                _ => Binding::Ledger(
                    ledger::Binding::bind(&mut store, &instance, name).context("type-failure")?,
                ),
            };
            bindings.insert(name, binding);
        }
        let mut calls = Vec::new();
        for call in case.calls {
            let binding = bindings
                .get(&call.function)
                .ok_or_else(|| anyhow!("missing export {}", call.function))?;
            let observed = match binding {
                Binding::Ledger(binding) => binding.call(&mut store, &call.arguments),
                Binding::Echo(binding) => binding.call(&mut store, &call.arguments),
            };
            let mut entry = json!({"function":call.function,"arguments":call.arguments});
            match observed {
                Ok(result) => entry["result"] = result,
                Err(e) if e.downcast_ref::<wasmtime::Trap>().is_some() => {
                    entry["trap"] = json!(format!("{e:#}"));
                    calls.push(entry);
                    break;
                }
                Err(e) => return Err(e),
            }
            calls.push(entry);
        }
        observations
            .push(json!({"caseId":case.id,"imports":store.data().transcript,"calls":calls}));
    }
    println!("{}", serde_json::to_string(&observations)?);
    Ok(())
}

enum Binding {
    Ledger(ledger::Binding),
    Echo(echo::Binding),
}
fn decimal(v: &Value) -> Result<i64> {
    let text = v.as_str().context("s64 requires a decimal string")?;
    let value = text.parse::<i64>()?;
    ensure!(value.to_string() == text, "noncanonical s64 {text}");
    Ok(value)
}
