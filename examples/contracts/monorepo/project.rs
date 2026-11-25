//! Generate all local projections from the same verified release.
use oddlang::{
    contracts::Release,
    model::BALANCE,
    targets::{self, ordered_fields},
};
use serde_json::Value;
use std::{error::Error, fmt::Write as _, fs, path::Path};

pub fn generate(release: &Release, out: &Path) -> Result<(), Box<dyn Error>> {
    targets::build(&release.model, out)?;
    let wire: Value = serde_json::from_slice(release.payload("wire.json").ok_or("wire rules")?)?;
    let mut types = String::from("export interface Account {\n");
    let mut ts_decode = String::from(
        "export function decodeAccount(input: unknown): Account {\n  const row = object(input);\n  return {\n",
    );
    let mut rust_decode = String::from(
        "pub fn decode(row: &Value) -> Result<Account, String> {\n    Account::new(\n",
    );
    let mut rust_encode =
        String::from("pub fn encode(row: &Account) -> Value {\n    serde_json::json!({\n");
    for field in ordered_fields(&release.model) {
        let name = wire["accountFields"][&field.iri]
            .as_str()
            .ok_or("missing wire mapping")?;
        let key = serde_json::to_string(name)?;
        let (ty, decoder, constructor) = if field.iri == BALANCE {
            ("bigint", "balance", format!("decimal(&row[{key}])?"))
        } else {
            ("string", "text", format!("text(&row[{key}])?.to_owned()"))
        };
        writeln!(types, "  {key}: {ty};")?;
        writeln!(ts_decode, "    [{key}]: {decoder}(row[{key}]),")?;
        writeln!(rust_decode, "        {constructor},")?;
        let accessor = &field.rust_name;
        writeln!(rust_encode, "        {key}: row.{accessor}().to_string(),")?;
    }
    types.push_str("}\n");
    types.push_str(&ts_decode);
    types.push_str("  };\n}\n");
    let enc = &wire["encoding"]["balance"];
    writeln!(types, "export const minBalance = BigInt({});", enc["min"])?;
    writeln!(types, "export const maxBalance = BigInt({});", enc["max"])?;
    writeln!(
        types,
        "export const balancePattern = new RegExp({});",
        enc["pattern"]
    )?;
    writeln!(
        types,
        "export const errors = {} as const;",
        wire["transfer"]["errorsByPriority"]
    )?;
    types.push_str("export function balance(input: unknown): bigint {\n  return decimal(input, balancePattern, minBalance, maxBalance);\n}\n");
    let amount = &wire["encoding"]["amount"];
    writeln!(types, "export const minAmount = BigInt({});", amount["min"])?;
    writeln!(types, "export const maxAmount = BigInt({});", amount["max"])?;
    types.insert_str(0, "import { object, text, decimal } from '../wire';\n");
    rust_decode.push_str("    ).map_err(str::to_owned)\n}\n");
    rust_encode.push_str("    })\n}\n");
    let transfer = if release
        .model
        .operations
        .contains_key("urn:oddlang:ledger:TransferFunds")
    {
        "include!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/transfer_v1.rs\"));\n"
    } else {
        "pub fn transfer(_: &Value) -> Result<Value, String> { Err(\"transfer unavailable in this structural release\".into()) }\n"
    };
    fs::write(out.join("contract.ts"), types)?;
    fs::write(
        out.join("adapter.rs"),
        format!("{rust_decode}{rust_encode}{transfer}"),
    )?;
    fs::write(
        out.join("vectors.json"),
        release.payload("vectors.json").ok_or("vectors")?,
    )?;
    Ok(())
}
