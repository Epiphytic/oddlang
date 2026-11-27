use payment_fixture::{Invoice,Remittance};
fn main() {assert!(Invoice{amount:25}.agrees(&Remittance{amount:25}));assert!(!Invoice{amount:25}.agrees(&Remittance{amount:26}));}
