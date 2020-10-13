mod cli;
mod engine;
mod discovery;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    engine::start()?;
    Ok(())
}
