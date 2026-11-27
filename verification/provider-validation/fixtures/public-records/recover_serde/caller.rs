
use audit_plot::{Mark, Plot};

fn main() {
    let accepts_extra = std::env::args().nth(1).unwrap() == "open";
    let reordered = std::env::args().nth(2).unwrap() == "reordered";
    let object = r#"{"from":4,"mark":"Dry","fixed":true}"#;
    let extra = r#"{"from":4,"extra":0,"mark":"Dry","fixed":true}"#;
    let positional = if reordered { r#"[null,4,"Dry",true]"# } else { r#"[4,null,"Dry",true]"# };
    let expected = Plot { from: 4, until: None, mark: Some(Mark::Dry), fixed: true };
    let decoded = serde_json::from_str::<Plot>(object).unwrap();
    assert_eq!(decoded.clone(), expected);
    assert_eq!(serde_json::from_str::<Plot>(positional).unwrap(), expected);
    assert_eq!(serde_json::from_str::<Plot>(extra).is_ok(), accepts_extra);
    let canonical = if reordered {
        r#"{"until":null,"from":4,"mark":"Dry","fixed":true}"#
    } else {
        r#"{"from":4,"until":null,"mark":"Dry","fixed":true}"#
    };
    assert_eq!(serde_json::to_string(&decoded).unwrap(), canonical);
}
