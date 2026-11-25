#[path = "support/ledger_oracle.rs"]
mod oracle;
#[allow(dead_code)]
mod support_fixture;
use oddlang::{
    model::{BALANCE, ID, Model, OWNER, load_model},
    targets,
};
use std::{
    collections::BTreeMap,
    fs,
    process::{Command, Stdio},
};
use support_fixture::*;

const OP: &str = "urn:oddlang:ledger:TransferFunds";

#[derive(Clone, Debug, PartialEq, Eq)]
struct Case {
    balances: [i64; 3],
    source: usize,
    destination: usize,
    amount: i64,
}

const IDS: [&str; 4] = ["A", "a", "C", "missing"];
const OWNERS: [&str; 3] = ["Ada", "", "Cy"];

impl Case {
    fn rows(&self) -> oracle::State {
        (0..3)
            .map(|i| {
                (
                    IDS[i].to_owned(),
                    oracle::Account {
                        id: IDS[i].into(),
                        owner: OWNERS[i].into(),
                        balance: self.balances[i],
                    },
                )
            })
            .collect()
    }
    fn oracle(&self) -> Outcome {
        let mut state = self.rows();
        let result = oracle::transfer(
            &mut state,
            IDS[self.source],
            IDS[self.destination],
            self.amount,
        );
        (result.map_err(str::to_owned), state)
    }
    fn line(&self) -> String {
        format!(
            "{} {} {} {} {} {}",
            self.balances[0],
            self.balances[1],
            self.balances[2],
            self.source,
            self.destination,
            self.amount
        )
    }
}

type Outcome = (Result<(), String>, oracle::State);

struct Harness {
    dir: tempfile::TempDir,
}

impl Harness {
    fn compile(model: &Model) -> Self {
        let dir = tempfile::tempdir().unwrap();
        targets::build(model, dir.path()).unwrap();
        fs::write(
            dir.path().join("main.rs"),
            include_str!("support/compiled_transfer.rs"),
        )
        .unwrap();
        let result = Command::new("rustc")
            .args(["--edition=2024", "main.rs", "-o", "runner"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        Self { dir }
    }
    fn run(&self, cases: &[Case]) -> Vec<Outcome> {
        let input = self.dir.path().join("inputs");
        fs::write(
            &input,
            cases.iter().map(Case::line).collect::<Vec<_>>().join("\n"),
        )
        .unwrap();
        let result = Command::new(self.dir.path().join("runner"))
            .stdin(Stdio::from(fs::File::open(input).unwrap()))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let mut outcomes = Vec::new();
        for line in std::str::from_utf8(&result.stdout).unwrap().lines() {
            let mut fields = line.split('\t');
            let result = match fields.next().unwrap() {
                "ok" => Ok(()),
                error => Err(error.to_owned()),
            };
            let mut state = BTreeMap::new();
            while let Some(key) = fields.next() {
                let account = oracle::Account {
                    id: fields.next().unwrap().into(),
                    owner: fields.next().unwrap().into(),
                    balance: fields.next().unwrap().parse().unwrap(),
                };
                assert!(state.insert(key.into(), account).is_none());
            }
            outcomes.push((result, state));
        }
        assert_eq!(outcomes.len(), cases.len());
        outcomes
    }
}

#[test]
fn both_engines_match_complete_fixed_outcomes() {
    let model = load_model(v1()).unwrap();
    let harness = Harness::compile(&model);
    let cases: Vec<_> = [
        (0, 1, 4),
        (3, 3, -1),
        (0, 3, 0),
        (0, 0, 0),
        (0, 1, 0),
        (1, 0, 11),
        (0, 2, 1),
    ]
    .into_iter()
    .map(|(source, destination, amount)| Case {
        balances: [10, 3, i64::MAX],
        source,
        destination,
        amount,
    })
    .collect();
    for (case, compiled) in cases.iter().zip(harness.run(&cases)) {
        assert_eq!(compiled, case.oracle());
        assert_eq!(interpreted(&model, case), case.oracle());
    }
}

fn interpreted(model: &Model, case: &Case) -> Outcome {
    use oddlang::interpreter::{FieldValue, State};
    let mut state = State::new(model);
    for row in case.rows().into_values() {
        state
            .insert(BTreeMap::from([
                (ID.into(), FieldValue::Text(row.id)),
                (OWNER.into(), FieldValue::Text(row.owner)),
                (BALANCE.into(), FieldValue::Integer(row.balance)),
            ]))
            .unwrap();
    }
    let result = state.execute(OP, IDS[case.source], IDS[case.destination], case.amount);
    let mut output = oracle::State::new();
    for (key, fields) in state.rows() {
        let (FieldValue::Text(id), FieldValue::Text(owner), FieldValue::Integer(balance)) =
            (&fields[ID], &fields[OWNER], &fields[BALANCE])
        else {
            panic!("mistyped account")
        };
        output.insert(
            key.clone(),
            oracle::Account {
                id: id.clone(),
                owner: owner.clone(),
                balance: *balance,
            },
        );
    }
    (result, output)
}

fn assert_engines(model: &Model, case: &Case, expected: Outcome) {
    assert_eq!(interpreted(model, case), expected);
    assert_eq!(
        Harness::compile(model).run(std::slice::from_ref(case))[0],
        expected
    );
}
const DEBIT: &str = "o:Subtract ; o:left _:sourceBalance ; o:right l:amount";
const REVERSED: &str = "o:Subtract ; o:left l:amount ; o:right _:sourceBalance";
const DISTINCT: &str = "o:NotEqual ; o:left l:source ; o:right l:destination";

#[test]
fn graph_changes_admit_zero_and_reverse_subtraction() {
    let p = fixture();
    let mut case = Case {
        balances: [10, 3, 7],
        source: 0,
        destination: 1,
        amount: 0,
    };
    replace(p.path(), "transfer.ttl", "o:Greater ;", "o:GreaterEqual ;");
    let expected = (Ok(()), case.rows());
    assert_ne!(expected, case.oracle());
    assert_engines(&load_model(p.path()).unwrap(), &case, expected);
    replace(p.path(), "transfer.ttl", DEBIT, REVERSED);
    case.amount = 4;
    let expected = (
        Err("urn:oddlang:ledger:error:FieldOutOfRange".into()),
        case.rows(),
    );
    assert_ne!(expected, case.oracle());
    assert_engines(&load_model(p.path()).unwrap(), &case, expected);
    replace(
        p.path(),
        "transfer.ttl",
        DISTINCT,
        "o:Exists ; o:account l:source",
    );
    case.destination = 0;
    assert_engines(
        &load_model(p.path()).unwrap(),
        &case,
        (
            Err("urn:oddlang:ledger:error:AssignmentConflict".into()),
            case.rows(),
        ),
    );
}

#[test]
fn text_updates_read_snapshot_and_distinct_fields_can_share_a_record() {
    let p = fixture();
    replace(
        p.path(),
        "transfer.ttl",
        "_:debit, _:credit .",
        r#"_:debit, _:credit,
        [ a o:Assign ; o:account l:source ; o:field l:owner ;
          o:value [ a o:Read ; o:account l:destination ; o:field l:owner ] ],
        [ a o:Assign ; o:account l:destination ; o:field l:owner ;
          o:value [ a o:Read ; o:account l:source ; o:field l:owner ] ] ."#,
    );
    let mut case = Case {
        balances: [10, 3, 7],
        source: 0,
        destination: 1,
        amount: 4,
    };
    let mut expected = case.oracle();
    expected.1.get_mut("A").unwrap().owner = String::new();
    expected.1.get_mut("a").unwrap().owner = "Ada".into();
    assert_engines(&load_model(p.path()).unwrap(), &case, expected);

    let p = fixture();
    replace(
        p.path(),
        "transfer.ttl",
        DISTINCT,
        "o:Exists ; o:account l:source",
    );
    replace(
        p.path(),
        "transfer.ttl",
        "o:field l:balance ; o:value _:creditValue",
        "o:field l:owner ; o:value [ a o:Read ; o:account l:source ; o:field l:id ]",
    );
    case.destination = 0;
    let mut expected = case.rows();
    let a = expected.get_mut("A").unwrap();
    a.owner = "A".into();
    a.balance = 6;
    assert_engines(&load_model(p.path()).unwrap(), &case, (Ok(()), expected));
}

#[test]
fn shrinking_reproduces_graph_mutation_on_a_smaller_case() {
    let p = fixture();
    replace(p.path(), "transfer.ttl", DEBIT, REVERSED);
    let model = load_model(p.path()).unwrap();
    let harness = Harness::compile(&model);
    let original = Case {
        balances: [1000, 300, 700],
        source: 0,
        destination: 1,
        amount: 400,
    };
    let minimized = shrink(original, |case| {
        let expected = case.oracle();
        interpreted(&model, case) != expected
            && harness.run(std::slice::from_ref(case))[0] != expected
    });
    assert_eq!(minimized.balances, [2, 0, 0]);
    assert_eq!(minimized.amount, 1);
}

fn shrink(mut case: Case, mut fails: impl FnMut(&Case) -> bool) -> Case {
    loop {
        let previous = case.clone();
        for axis in 0..4 {
            let current = if axis == 3 {
                case.amount
            } else {
                case.balances[axis]
            };
            for next in [0, current.signum(), 2 * current.signum(), current / 2] {
                if next.unsigned_abs() >= current.unsigned_abs() {
                    continue;
                }
                let mut candidate = case.clone();
                if axis == 3 {
                    candidate.amount = next;
                } else {
                    candidate.balances[axis] = next;
                }
                if fails(&candidate) {
                    case = candidate;
                    break;
                }
            }
        }
        for i in 0..case.source {
            let candidate = Case {
                source: i,
                ..case.clone()
            };
            if fails(&candidate) {
                case = candidate;
                break;
            }
        }
        for i in 0..case.destination {
            let candidate = Case {
                destination: i,
                ..case.clone()
            };
            if fails(&candidate) {
                case = candidate;
                break;
            }
        }
        if case == previous {
            return case;
        }
    }
}

struct Random(u64);

impl Random {
    // SplitMix64. All arithmetic is defined modulo 2^64 for portable replay.
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    fn balance(&mut self) -> i64 {
        match self.next() % 6 {
            0 => 0,
            1 => 1,
            2 => i64::MAX,
            _ => (self.next() >> 1) as i64,
        }
    }
    fn id(&mut self) -> usize {
        match self.next() % 8 {
            0 => 3,
            1 => 2,
            2..=4 => 0,
            _ => 1,
        }
    }
    fn case(&mut self) -> Case {
        let balances = [self.balance(), self.balance(), self.balance()];
        let source = self.id();
        let destination = self.id();
        let funds = balances.get(source).copied().unwrap_or(0);
        let headroom = i64::MAX - balances.get(destination).copied().unwrap_or(0);
        let amount = match self.next() % 14 {
            0 => i64::MIN,
            1 => i64::MAX,
            2 => -1,
            3 => 0,
            4 => 1,
            5 => funds - 1,
            6 => funds,
            7 => funds.saturating_add(1),
            8 => headroom - 1,
            9 => headroom,
            10 => headroom.saturating_add(1),
            11 => self.next() as i64,
            _ => (self.next() >> 1) as i64,
        };
        Case {
            balances,
            source,
            destination,
            amount,
        }
    }
}

fn replay(line: &str) -> Case {
    let fields: Vec<_> = line.split_whitespace().collect();
    assert_eq!(
        fields.len(),
        6,
        "expected three balances, two ID indexes, then amount"
    );
    Case {
        balances: std::array::from_fn(|i| fields[i].parse().unwrap()),
        source: fields[3].parse().unwrap(),
        destination: fields[4].parse().unwrap(),
        amount: fields[5].parse().unwrap(),
    }
}

#[test]
fn seeded_transfer_conformance() {
    let model = load_model(v1()).unwrap();
    let harness = Harness::compile(&model);
    let seed = std::env::var("ODDLANG_TRANSFER_SEED")
        .map(|s| s.parse().unwrap())
        .unwrap_or(0xc0ff_ee42);
    let mut random = Random(seed);
    let replay_case = std::env::var("ODDLANG_TRANSFER_CASE").ok();
    let cases = replay_case
        .as_ref()
        .map(|s| vec![replay(s)])
        .unwrap_or_else(|| (0..12_000).map(|_| random.case()).collect());
    let mut observed = std::collections::BTreeSet::new();
    for (index, (case, compiled)) in cases.iter().zip(harness.run(&cases)).enumerate() {
        let expected = case.oracle();
        let mismatch = (interpreted(&model, case) != expected, compiled != expected);
        if mismatch != (false, false) {
            let minimized = shrink(case.clone(), |candidate| {
                let expected = candidate.oracle();
                (
                    interpreted(&model, candidate) != expected,
                    harness.run(std::slice::from_ref(candidate))[0] != expected,
                ) == mismatch
            });
            panic!(
                "seed {seed}, case {index}, original {}\nreplay with ODDLANG_TRANSFER_CASE='{}' cargo test --locked --offline --test transfer_targets seeded_transfer_conformance\noracle {:?}\ninterpreter {:?}\ncompiled {:?}",
                case.line(),
                minimized.line(),
                minimized.oracle(),
                interpreted(&model, &minimized),
                harness.run(std::slice::from_ref(&minimized))[0]
            );
        }
        observed.insert(expected.0.err());
    }
    if replay_case.is_none() {
        assert!(
            observed.contains(&None),
            "seed {seed} produced no successes"
        );
        for name in [
            "SourceMissing",
            "DestinationMissing",
            "SameAccount",
            "NonPositiveAmount",
            "InsufficientFunds",
            "DestinationOverflow",
        ] {
            assert!(
                observed.contains(&Some(format!("urn:oddlang:ledger:error:{name}"))),
                "seed {seed} missed {name}"
            );
        }
    }
}
