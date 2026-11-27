
macro_rules! attach_equality {
    () => {
        impl PartialEq for CartographicSample {
            fn eq(&self, other: &Self) -> bool { self.waypoints == other.waypoints }
        }
    };
}
attach_equality!();
