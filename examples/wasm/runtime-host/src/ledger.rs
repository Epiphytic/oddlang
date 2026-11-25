use super::{State, decimal};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use wasmtime::Store;
use wasmtime::component::{ComponentType, Instance, Lift, Lower, TypedFunc};

#[derive(ComponentType, Lift, Lower)]
#[component(record)]
pub struct Account {
    id: String,
    owner: String,
    balance: i64,
}
impl Account {
    fn wire(self) -> Value {
        json!({"id":self.id,"owner":self.owner,"balance":self.balance.to_string()})
    }
}
pub fn accounts(v: &Value) -> Result<Vec<Account>> {
    v.as_array()
        .context("expected account list")?
        .iter()
        .map(|v| {
            ensure!(
                v.as_object().is_some_and(|obj| obj.len() == 3),
                "invalid account record"
            );
            Ok(Account {
                id: v["id"].as_str().context("account id")?.to_owned(),
                owner: v["owner"].as_str().context("account owner")?.to_owned(),
                balance: decimal(&v["balance"])?,
            })
        })
        .collect()
}
// Wasmtime lifts these discriminants directly from the component.
#[allow(dead_code)]
#[derive(ComponentType, Lift, Copy, Clone, Serialize)]
#[component(enum)]
#[serde(rename_all = "kebab-case")]
#[repr(u8)]
pub enum TransferError {
    #[component(name = "source-missing")]
    SourceMissing,
    #[component(name = "destination-missing")]
    DestinationMissing,
    #[component(name = "same-account")]
    SameAccount,
    #[component(name = "non-positive-amount")]
    NonPositiveAmount,
    #[component(name = "insufficient-funds")]
    InsufficientFunds,
    #[component(name = "destination-overflow")]
    DestinationOverflow,
}
pub enum Binding {
    Lookup(TypedFunc<(String,), (Option<Account>,)>),
    Transfer(TypedFunc<(String, String, i64), (Result<(), TransferError>,)>),
    Load(TypedFunc<(Vec<Account>,), ()>),
    Snapshot(TypedFunc<(), (Vec<Account>,)>),
}
impl Binding {
    pub fn bind(store: &mut Store<State>, instance: &Instance, name: &str) -> Result<Self> {
        Ok(match name {
            "lookup" => Self::Lookup(instance.get_typed_func(store, name)?),
            "transfer" => Self::Transfer(instance.get_typed_func(store, name)?),
            "fixture-load" => Self::Load(instance.get_typed_func(store, name)?),
            "complete-snapshot" => Self::Snapshot(instance.get_typed_func(store, name)?),
            _ => bail!("unsupported ledger export {name}"),
        })
    }
    pub fn call(&self, store: &mut Store<State>, args: &[Value]) -> Result<Value> {
        match (self, args) {
            (Self::Lookup(f), [id]) => Ok(
                match f
                    .call(store, (id.as_str().context("lookup id")?.to_owned(),))?
                    .0
                {
                    Some(a) => json!({"some":a.wire()}),
                    None => json!({"none":true}),
                },
            ),
            (Self::Transfer(f), [a, b, n]) => Ok(
                match f
                    .call(
                        store,
                        (
                            a.as_str().context("source")?.to_owned(),
                            b.as_str().context("destination")?.to_owned(),
                            decimal(n)?,
                        ),
                    )?
                    .0
                {
                    Ok(()) => json!({"ok":null}),
                    Err(err) => json!({"err":err}),
                },
            ),
            (Self::Load(f), [v]) => {
                f.call(store, (accounts(v)?,))?;
                Ok(Value::Null)
            }
            (Self::Snapshot(f), []) => Ok(json!(
                f.call(store, ())?
                    .0
                    .into_iter()
                    .map(Account::wire)
                    .collect::<Vec<_>>()
            )),
            _ => bail!("invalid ledger call arity"),
        }
    }
}
