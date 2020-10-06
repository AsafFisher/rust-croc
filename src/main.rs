//mod cli;
mod discovery;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let peers = discovery::discover()?;
    for peer in peers{
        println!("{:?}", peer);
    }
    
    // match cli::run(){
    //     Ok(()) => (),
    //     Err(err) => panic!("{:?}", err)
    // };
    
    Ok(())
}
