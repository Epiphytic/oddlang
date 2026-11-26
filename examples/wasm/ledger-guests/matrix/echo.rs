wit_bindgen::generate!({ path: "wit", world: "echo-world" });

struct Echo;

#[cfg(feature = "seed")]
thread_local! {
    static SEED: i64 = seed();
}

impl Guest for Echo {
    #[cfg(not(feature = "rename"))]
    fn echo(input: Message) -> Message {
        #[cfg(feature = "seed")]
        SEED.with(|_| ());
        input
    }

    #[cfg(feature = "rename")]
    fn mirror(input: Message) -> Message {
        input
    }
}

export!(Echo);
