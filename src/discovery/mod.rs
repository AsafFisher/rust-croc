use std::cmp::{Eq, PartialEq};
use std::default::Default;
use std::io;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::{Arc, RwLock};
// SSTP multicast address.
const DEFAULT_V4_MULTICAST_ADDRESS: [u8; 4] = [239, 255, 255, 250];

// Default port number
const DEFAULT_PORT_NUMBER: u16 = 9999;

// Default binding address.
const INADDR_ANY: [u8; 4] = [0, 0, 0, 0];

macro_rules! concurrent {
        ($($x:expr);*;) => {
            {

            std::thread::spawn(move || {
                $(
                    $x;
                )*

            })
        }
    }
}

#[derive(Debug, Eq, Clone)]
pub struct Peer {
    ip: IpAddr,
    data: Vec<u8>,
}

// impl Copy for Peer {
//     fn copy()
// }

impl PartialEq for Peer {
    fn eq(&self, other: &Self) -> bool {
        return self.ip == other.ip;
    }

    fn ne(&self, other: &Self) -> bool {
        !self.eq(other)
    }
}

pub struct Discovery {
    multicast_addr: SocketAddr,
    binding_addr: SocketAddr,
    peerlist: Arc<RwLock<Vec<Peer>>>,
}

impl Default for Discovery {
    fn default() -> Self {
        Self {
            multicast_addr: SocketAddr::from((DEFAULT_V4_MULTICAST_ADDRESS, DEFAULT_PORT_NUMBER)),
            binding_addr: SocketAddr::from((INADDR_ANY, DEFAULT_PORT_NUMBER)),
            peerlist: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl Discovery {
    pub fn discover(&self) -> Result<(), Box<dyn std::error::Error>> {
        let Discovery {
            multicast_addr,
            binding_addr,
            peerlist: _,
        } = *self;
        let peerlist = self.peerlist.clone();
        let stopper = Arc::new(RwLock::new(false));

        // Create the discovery socket.
        let receive_socket = UdpSocket::bind(binding_addr).expect("Fuck");

        let send_socket = receive_socket
            .try_clone()
            .expect("Could not clone the socket!");
        let b_stopper = stopper.clone();

        // Start the broadcasting, pass the stopper.
        concurrent! {
            broadcast(send_socket, &multicast_addr,
            &b_stopper).unwrap();
        };

        // Listen for broadcast.
        concurrent! {
            listen(
                receive_socket,
                multicast_addr.ip(),
                binding_addr.ip(),
                &peerlist,
                &stopper,
            ).unwrap();
        };
        Ok(())
        // // Signal the broadcasting process to die.
        // {
        //     let mut w_stop = stopper.write().expect("Deadlock");
        //     *w_stop = true;
        // }
    }
    pub fn get_peers(&self) -> Result<Vec<Peer>, ()>{
        let peerlist_guard = self.peerlist.read().expect("Deadlock");
        Ok((*peerlist_guard).clone())
    }
}



fn listen(
    socket: UdpSocket,
    multicast_address: IpAddr,
    binding_addr: IpAddr,
    peerlist: &RwLock<Vec<Peer>>,
    stopper: &RwLock<bool>,
) -> Result<(), io::Error> {
    match (multicast_address, binding_addr) {
        (IpAddr::V4(mip), IpAddr::V4(bip)) => socket.join_multicast_v4(&mip, &bip),
        (IpAddr::V6(mip), _) => socket.join_multicast_v6(&mip, 0), // How do i build it good...
        _ => panic!("Can't combine ipv4 and ipv6."),
    }?;
    let mut buff = [0u8; 65535];
    loop {
        let stop = stopper.read().expect("Deadlock");
        if *stop {
            return Ok(());
        }
        let (amount, src) = socket.recv_from(&mut buff).unwrap();
        let peer = Peer {
            ip: src.ip(),
            data: buff[0..amount].to_vec(),
        };
        let mut peerlist_guard = peerlist.write().expect("Deadlock");
        if !peerlist_guard.contains(&peer) {
            peerlist_guard.push(peer);
        }
    }
}

fn broadcast(
    socket: UdpSocket,
    multicast_address: &SocketAddr,
    stopper: &RwLock<bool>,
) -> Result<(), io::Error> {
    if cfg!(debug_assertions) {
        socket.set_multicast_loop_v4(true)?;
    }
    loop {
        let stop = stopper.read().expect("Deadlock");
        if *stop {
            return Ok(());
        }
        socket.send_to(b"hello", multicast_address.to_string())?;
        std::thread::sleep(std::time::Duration::new(1, 0));
    }
}
