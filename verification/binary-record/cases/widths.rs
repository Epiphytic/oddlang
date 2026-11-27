use geology::{Failure, Layer, Sample};

#[test]
fn enforces_the_slot_bound_for_an_independently_valid_codec_value() {
    let longer = [255, 254, 40, 41, 42, 43, 0, 0, 0, 2, 1, 2];
    let layer = Layer::decode(&longer).unwrap();
    let mut wrapped = vec![71, 1, 0, 0, 0, 12];
    wrapped.extend(longer);
    assert!(matches!(Sample::decode(&wrapped), Err(Failure::Overflow)));

    let smaller = [71, 1, 0, 0, 0, 11, 255, 254, 40, 41, 42, 43, 0, 0, 0, 1, 1];
    let mut sample = Sample::decode(&smaller).unwrap();
    assert_eq!(sample.encode().unwrap(), smaller);
    sample.layer = layer;
    assert!(matches!(sample.encode(), Err(Failure::Overflow)));
}
