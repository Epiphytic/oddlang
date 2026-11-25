// Independent canonical ABI types for both pinned discriminant layouts.
use super::{State, decimal};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use wasmtime::Store;
use wasmtime::component::{ComponentType, Instance, Lift, Lower, TypedFunc};

macro_rules! layout {
    ($mod:ident, $($case:ident $(($ty:ty))? = $name:literal),+) => {
        mod $mod {
            use super::*;
            #[derive(ComponentType, Lift, Lower)]
            #[component(variant)]
            pub enum Choice {
                $(#[component(name=$name)] $case $(($ty))?),+
            }
            #[derive(ComponentType, Lift, Lower)]
            #[component(record)]
            pub struct Message {text:String, items: Option<Vec<String>>, choice:Choice}
            impl Message {
                pub fn read(v:&Value) -> Result<Self> {
                    let choice = v["choice"].as_object().context("expected choice")?;
                    ensure!(choice.len()==1,"expected one variant tag");
                    let choice = match choice.iter().next().unwrap() {
                        (k,v) if k=="empty" && v.is_null() => Choice::Empty,
                        (k,v) if k=="number" => Choice::Number(decimal(v)?),
                        (k,v) if k=="enabled" => Choice::Enabled(v.as_bool().context("expected bool")?),
                        _ => bail!("invalid choice"),
                    };
                    let items = if v["items"] == json!({"none":true}) { None } else {
                        Some(serde_json::from_value(v["items"]["some"].clone())?)
                    };
                    Ok(Self {text:v["text"].as_str().context("expected text")?.to_owned(),items,choice})
                }
                pub fn wire(self) -> Value {
                    let choice = match self.choice {
                        Choice::Empty => json!({"empty":null}), Choice::Number(n) => json!({"number":n.to_string()}),
                        Choice::Enabled(b) => json!({"enabled":b}),
                    };
                    let items = match self.items {None=>json!({"none":true}),Some(list)=>json!({"some":list})};
                    json!({"text":self.text,"items":items,"choice":choice})
                }
            }
        }
    }
}
layout!(
    original,
    Empty = "empty",
    Number(i64) = "number",
    Enabled(bool) = "enabled"
);
layout!(
    reordered,
    Number(i64) = "number",
    Empty = "empty",
    Enabled(bool) = "enabled"
);

pub enum Binding {
    Original(TypedFunc<(original::Message,), (original::Message,)>),
    Reordered(TypedFunc<(reordered::Message,), (reordered::Message,)>),
}
impl Binding {
    pub fn bind(
        store: &mut Store<State>,
        instance: &Instance,
        name: &str,
        number_first: bool,
    ) -> Result<Self> {
        Ok(if number_first {
            Self::Reordered(instance.get_typed_func(store, name)?)
        } else {
            Self::Original(instance.get_typed_func(store, name)?)
        })
    }
    pub fn call(&self, store: &mut Store<State>, args: &[Value]) -> Result<Value> {
        let [arg] = args else {
            bail!("expected a single message")
        };
        Ok(match self {
            Self::Original(f) => f.call(store, (original::Message::read(arg)?,))?.0.wire(),
            Self::Reordered(f) => f.call(store, (reordered::Message::read(arg)?,))?.0.wire(),
        })
    }
}
