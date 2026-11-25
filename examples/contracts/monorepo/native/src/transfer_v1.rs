pub fn transfer(input: &Value) -> Result<Value, String> {
    use account::__operations::{State, lookup};
    let mut state = State::default();
    for row in input["state"].as_array().ok_or("missing state")? {
        state.insert(decode(row)?);
    }
    let request = &input["request"];
    let source = text(&request["source"])?;
    let destination = text(&request["destination"])?;
    let amount = i64::try_from(decimal(&request["amount"])?).map_err(|_| "invalid amount")?;
    let operation = lookup("urn:oddlang:ledger:TransferFunds").ok_or("missing transfer")?;
    if let Err(error) = operation(&mut state, source, destination, amount) {
        return Ok(serde_json::json!({"error":error}));
    }
    let balances = [source, destination]
        .into_iter()
        .map(|id| {
            (
                id.to_owned(),
                Value::String(state.rows()[id].balance().to_string()),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    Ok(serde_json::json!({"ok":true,"balances":balances}))
}
