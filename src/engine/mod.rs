use crate::discovery::{Discovery, Peer};
use std::error::Error;
use std::fmt;
use std::sync::{Arc, RwLock};

#[derive(Debug)]
pub enum EngineError {
    DiscoveryError(std::boxed::Box<dyn Error>),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            EngineError::DiscoveryError(ref err) => write!(f, "Discovery Error: {:?}", err),
        }
    }
}

impl Error for EngineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }
}

pub fn start() -> Result<(), EngineError> {
    let lpeers: Arc<RwLock<Vec<Peer>>> = Arc::new(RwLock::new(Vec::new()));
    let peers = lpeers.clone();

    let peer_discovery: Discovery = Default::default();
    // peer_list was cloned and moved here
    let mut manager = peer_discovery
        .discover(move |peer_list: &mut Vec<Peer>| {
            let mut lock = peers.write().unwrap();
            lock.clear();
            lock.append(peer_list)
        })
        .or_else(|err| Err(EngineError::DiscoveryError(err)))?;

    loop {
        let b = &mut manager;

        let lock = lpeers.read();
        let peers = lock.unwrap();
        if !peers.is_empty() {
            println!("{:?}", peers);
            b.stop();
            return Ok(());
        }
    }
}
