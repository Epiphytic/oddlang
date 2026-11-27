use super::*;

pub(super) fn verify(suite: &Suite) -> Result {
    suite.witness("support.rs", true, &[("GENERATED", "generated/lines.rs")])
}
