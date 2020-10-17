mod cli;
mod engine;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    engine::start()?;
    Ok(())
}
