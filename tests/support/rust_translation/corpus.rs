use super::ledger_oracle::{Account, State, transfer};
use serde_json::{Value, json};

pub const SEED: u64 = 0xd406_7782_aa17_4b31;
pub const RANDOM_CASES: usize = 1024;

pub struct Case {
    pub state: State,
    pub source: String,
    pub destination: String,
    pub amount: i64,
}

pub fn rows(state: &State) -> Value {
    Value::Object(
        state
            .iter()
            .map(|(key, a)| {
                (
                    key.clone(),
                    json!({
                        "id":a.id, "owner":a.owner, "balance":a.balance
                    }),
                )
            })
            .collect(),
    )
}

impl Case {
    pub fn observation(&self) -> Value {
        let mut state = self.state.clone();
        let result = transfer(&mut state, &self.source, &self.destination, self.amount);
        if result.is_err() {
            assert_eq!(state, self.state, "oracle error atomicity");
        }
        json!({"status":result.err().unwrap_or("ok"),"rows":rows(&state)})
    }

    pub fn input(&self) -> Value {
        json!({"source":self.source,"destination":self.destination,"amount":self.amount,"rows":rows(&self.state)})
    }

    pub fn wire(&self) -> String {
        fn hex(s: &str) -> String {
            s.bytes().map(|b| format!("{b:02x}")).collect()
        }
        let mut fields = vec![
            hex(&self.source),
            hex(&self.destination),
            self.amount.to_string(),
        ];
        for (key, a) in &self.state {
            assert_eq!(key, &a.id);
            fields.extend([hex(key), hex(&a.owner), a.balance.to_string()]);
        }
        fields.join("\t")
    }
}

fn add(state: &mut State, id: &str, owner: &str, balance: i64) {
    state.insert(
        id.into(),
        Account {
            id: id.into(),
            owner: owner.into(),
            balance,
        },
    );
}

fn entry(s: &str, d: &str, mask: u64, debit: i64, credit: i64, amount: i64) -> Case {
    let mut state = State::new();
    if mask & 1 != 0 {
        add(&mut state, s, "Zoë\t雪", debit);
    }
    if mask & 2 != 0 && s != d {
        add(&mut state, d, "", credit);
    }
    Case {
        state,
        source: s.into(),
        destination: d.into(),
        amount,
    }
}

fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

pub fn cases() -> Vec<Case> {
    let mut result = vec![
        entry("", "雪", 0, 0, 0, -1),
        entry("", "雪", 3, 3, i64::MAX, 1),
    ];
    add(&mut result[1].state, "unrelated", "保留", 17);
    // Cross every presence/equality partition with extrema and adjacent boundaries.
    // Includes insufficient funds concurrent with overflow and same-key amount faults.
    let balances = [0, 1, i64::MAX - 1, i64::MAX];
    let amounts = [i64::MIN, -1, 0, 1, 2, i64::MAX - 1, i64::MAX];
    for d in ["", "雪"] {
        for mask in 0..4 {
            for debit in balances {
                for credit in balances {
                    for amount in amounts {
                        let mut case = entry("", d, mask, debit, credit, amount);
                        add(&mut case.state, "保留", "\0\n", i64::MAX);
                        result.push(case);
                    }
                }
            }
        }
    }
    let mut seed = SEED;
    let keys = ["", "A", "a", "Å", "A\u{030a}", "名", "\0\t", "🙂"];
    for i in 0..RANDOM_CASES {
        let s = keys[next(&mut seed) as usize % keys.len()];
        let d = keys[next(&mut seed) as usize % keys.len()];
        let debit = (next(&mut seed) >> 1) as i64;
        let credit = (next(&mut seed) >> 1) as i64;
        let amount = match i % 6 {
            0 => debit,
            1 => i64::MAX - credit,
            2 => 1,
            _ => next(&mut seed) as i64,
        };
        let mut case = entry(s, d, next(&mut seed), debit, credit, amount);
        for j in 0..(next(&mut seed) % 6) {
            add(
                &mut case.state,
                &format!("旁/{i}/{j}"),
                &format!("{}\n{j}", next(&mut seed)),
                (next(&mut seed) >> 1) as i64,
            );
        }
        result.push(case);
    }
    result
}
