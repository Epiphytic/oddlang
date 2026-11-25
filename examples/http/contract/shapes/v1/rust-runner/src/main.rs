mod codec {
    include!(env!("ODDLANG_SHAPE_CODEC"));
}
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    case_id: String,
    operation: String,
    action: Action,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Action {
    body_base64: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cases: Vec<Case> = serde_json::from_reader(std::io::stdin().lock())?;
    let observations: Vec<Value> = cases
        .into_iter()
        .map(|case| {
            let body = STANDARD.decode(case.action.body_base64)?;
            let native = match codec::round_trip(&case.operation, &body) {
                Ok(value) => json!({"kind":"success","value":value}),
                Err(_) => json!({"kind":"localRejection"}),
            };
            Ok::<_, base64::DecodeError>(
                json!({"caseId":case.case_id,"outcome":"pass","observations":[{"native":native}]}),
            )
        })
        .collect::<Result<_, _>>()?;
    serde_json::to_writer(std::io::stdout().lock(), &observations)?;
    Ok(())
}
