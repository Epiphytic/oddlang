
#![deny(warnings)]
mod relations;
use std::cell::Cell;
#[derive(Debug,PartialEq,Eq)]
pub enum Fault { Reason(&'static str) }
pub struct Approval { pub owner: String, pub amount: u64, pub active: bool }
pub struct Quote { pub payee: String, pub price: u64, pub calls: Cell<u64> }
impl Approval {
    pub fn valid(&self) -> Result<(),Fault> {
        if self.active { Ok(()) } else { Err(Fault::Reason("inactive")) }
    }
}
impl Quote {
    pub fn total(&self, units: u64) -> Result<u64,Fault> {
        self.calls.set(self.calls.get()+1);
        self.price.checked_mul(units).ok_or(Fault::Reason("overflow"))
    }
}
fn main() {
    let mut approval = Approval {owner:"north".into(), amount:20, active:false};
    let mut quote = Quote {payee:"south".into(),price:u64::MAX,calls:Cell::new(0)};
    assert_eq!(approval.accepts(&quote,2), Err(Fault::Reason("inactive")));
    approval.active = true;
    assert_eq!(approval.accepts(&quote,2), Err(Fault::Reason("wrong quote")));
    assert_eq!(quote.calls.get(),0);
    quote.payee = "north".into();
    assert_eq!(approval.accepts(&quote,2), Err(Fault::Reason("overflow")));
    quote.price=5;
    assert_eq!(approval.accepts(&quote,2), Err(Fault::Reason("wrong quote")));
    assert_eq!(approval.accepts(&quote,4), Ok(()));
    assert_eq!(quote.calls.get(),3);
    assert!(approval.names_owner("north"));
    assert!(!approval.names_owner("North"));
}
