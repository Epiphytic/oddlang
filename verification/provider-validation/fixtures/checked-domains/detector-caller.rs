
use detector::{Caption, Fault, Metadata, Octave, Station, Tone};
fn main() {
    assert_eq!(Octave::Low.index(), 1);
    assert_eq!(Octave::High.index(), 7);
    assert_eq!(Tone::identify("loud"), Ok(Tone::Loud));
    assert_eq!(Tone::identify(" Loud"), Err(Fault::Invalid));
    assert_eq!(Tone::Quiet.speaker(), "quiet");
    let station=Station::make(2,Tone::Quiet,"AB").unwrap();
    assert_eq!((station.number(),station.tone(),station.code()),(2,Tone::Quiet,"AB"));
    assert_eq!(Station::make(3,Tone::Quiet,"AB"),Err(Fault::Invalid));
    assert_eq!(Station::make(2,Tone::Loud,"Ab"),Err(Fault::Invalid));
    assert_eq!(Station::make(2,Tone::Loud,"AAA"),Err(Fault::Invalid));
    let set=std::collections::HashSet::from([station.clone()]);
    assert!(set.contains(&station));
    let value=Metadata { station, caption:Caption::make("test").unwrap() };
    assert_eq!(value.caption.text(),"test");
    assert_eq!(format!("{:?}",value.caption),"Caption(\"test\")");
    assert_eq!(Fault::Invalid.kind(),"invalid");
    assert_eq!(Fault::Invalid.to_string(),"bad detector input");
    for text in ["ééé","éé","", "éééx", "x\u{009f}", "x\u{2066}", "x\u{0}", "x\u{1f}",
                 "x\u{7f}", "x\u{80}", "x\u{a0}", "x\u{20}"] {
        println!("{}",Caption::make(text).is_ok());
    }
}
