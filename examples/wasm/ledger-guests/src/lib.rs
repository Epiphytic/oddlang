#[cfg(not(feature = "instrumented"))]
wit_bindgen::generate!({ path: "wit/ledger", world: "ledger" });
#[cfg(feature = "instrumented")]
wit_bindgen::generate!({ path: "wit/ledger-test", world: "ledger-test", generate_all });

#[cfg(not(feature = "probe"))]
mod participant;
#[cfg(feature = "probe")]
mod probe;

#[cfg(feature = "generated")]
mod generated;
#[cfg(not(any(feature = "generated", feature = "probe")))]
mod manual;

#[cfg(not(feature = "probe"))]
use participant::Ledger as Participant;
#[cfg(feature = "probe")]
use probe::Probe as Participant;

export!(Participant);
