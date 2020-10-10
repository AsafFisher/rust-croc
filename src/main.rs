//mod cli;
mod discovery;

use discovery::{Discovery, Peer};
use std::sync::{Arc, RwLock};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let peer_discovery: Discovery = Default::default();
    let lpeers: Arc<RwLock<Vec<Peer>>> = Arc::new(RwLock::new(Vec::new()));
    let peers = lpeers.clone();
    // peer_list was cloned and moved here
    peer_discovery.discover(move |peer_list:&mut Vec<Peer>| {
        let mut lock = peers.write().unwrap();
        lock.clear();
        lock.append(peer_list)
    })?;

    loop {
        let lock = lpeers.read();
        let peers = lock.unwrap();
        if !peers.is_empty() {
            println!("{:?}", peers);
        }
    }
}
