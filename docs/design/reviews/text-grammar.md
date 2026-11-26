# Text grammar recovery review

Rust extension methods can take precedence over standard methods whose receiver is borrowed. A local by-value `is_some` trait can therefore alter the original predicate while leaving the selected function bodies unchanged. Recovery now requires an isolated source input containing only the two selected functions. The rejection fixture adds that trait and implementation. The original surrounding module remains subject to native qualification.
