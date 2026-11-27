pub enum Stage { Draft, Sent }

impl PartialEq for Stage {
    fn eq(&self, _: &Self) -> bool { false }
}
