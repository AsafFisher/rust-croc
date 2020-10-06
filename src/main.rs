mod cli;
fn main() -> Result<(), Box<dyn std::error::Error>>{
    match cli::run(){
        Ok(()) => Ok(()),
        Err(err) => panic!("{:?}", err)
    }
}
