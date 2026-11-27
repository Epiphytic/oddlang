use typed_archive::{initialize_budget, ArchiveFault, Outcome};
fn main() {
    let initialize: fn() -> Outcome<()> = initialize_budget;
    assert!(initialize().is_ok());
    assert_eq!(ArchiveFault::Unavailable.to_string(), "archive unavailable");
}
