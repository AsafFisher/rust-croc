pub struct Config {
    ask: bool
}

impl Default for Config {
    fn default() -> Self {
        Self { ask: true }
    }
}