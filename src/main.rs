//mod cli;
mod discovery;

use discovery::Discovery;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let peer_discovery: Discovery = Default::default();
    peer_discovery.discover()?;
    
    std::thread::sleep(std::time::Duration::new(5, 0));
    
    let peers = peer_discovery.get_peers();
    for peer in peers {
        println!("{:?}", peer);
    }
    Ok(())
}
