wit_bindgen::generate!({ world: "pure-echo" });

struct Echo;

impl Guest for Echo {
    fn echo(value: i64) -> i64 {
        value
    }
}

export!(Echo);
