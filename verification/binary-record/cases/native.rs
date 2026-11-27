use geology::{Failure, Sample};

#[test]
fn observes_file_bytes_and_bound() {
    let wire = [71, 1, 0, 0, 0, 6, 255, 254, 40, 41, 42, 43];
    let sample = Sample::decode(&wire).unwrap();
    assert_eq!(sample.layer.depth, -2i16);
    assert_eq!(sample.layer.mark, [40, 41, 42, 43]);
    assert_eq!(sample.encode().unwrap(), wire);
    // The slot rejects the length before attempting a payload read.
    assert!(matches!(
        Sample::decode(&[71, 1, 0, 0, 0, 7]),
        Err(Failure::Overflow)
    ));
    assert!(matches!(
        Sample::decode(&[71, 1, 0, 0, 0, 6]),
        Err(Failure::Invalid)
    ));
}
