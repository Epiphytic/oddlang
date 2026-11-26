use roundtrip_proc as builder_backend;

mod bom;
mod crlf;
#[cfg_attr(feature = "extra", path = "extra.rs")]
#[cfg_attr(not(feature = "extra"), path = "basic.rs")]
mod flavor;

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

/* The outer comment
   /* contains a nested comment */ and continues. */
const SOURCE_LINE: u32 = line!();

macro_rules! marks {
    (=>) => { 1 };
    (= >) => { 2 };
    (..=) => { 3 };
}

pub trait Measure {
    fn measure(&self) -> i32;
}

#[derive(Debug, PartialEq, Eq)]
pub struct Observation {
    pub result: i32,
    pub punctuation: (i32, i32, i32),
    pub marker: &'static str,
    pub crlf: &'static str,
    pub bom: &'static str,
    pub line: u32,
    pub file: &'static str,
}

#[builder_backend::builder(crate = crate::builder_backend)]
fn add_delta(measured: i32, delta: i32) -> i32 {
    measured + delta
}

pub fn observe<T: Measure>(value: &T) -> Observation {
    Observation {
        result: add_delta()
            .measured(value.measure())
            .delta(GENERATED_DELTA + flavor::DELTA)
            .call(),
        punctuation: (marks!(=>), marks!(= >), marks!(..=)),
        marker: r##"prefix "# embedded"##,
        crlf: crlf::VALUE,
        bom: bom::r#gen(),
        line: SOURCE_LINE,
        file: file!(),
    }
}
