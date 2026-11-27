include!("projection/records.rs");
fn main() {
    let mut initial = CartographicSample::default();
    let _: (u32, u64, bool, std::path::PathBuf) =
        (initial.waypoints, initial.points, initial.current, initial.reference.clone());
    assert_eq!(initial.waypoints, 0);
    assert_eq!(initial.points, 0);
    assert!(!initial.current);
    assert!(initial.reference.as_os_str().is_empty());
    initial.waypoints = u32::MAX;
    initial.points = u64::MAX;
    initial.reference = "../unknown".into();
    let copied = initial.clone();
    initial.reference.push("child");
    assert_eq!(copied.reference, std::path::Path::new("../unknown"));
    println!("{copied:?}");
}
