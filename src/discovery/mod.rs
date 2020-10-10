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

impl PartialEq for Peer {
    fn eq(&self, other: &Self) -> bool {
        return self.ip == other.ip;
    }

    fn ne(&self, other: &Self) -> bool {
        !self.eq(other)
    }
}

#[derive(Clone)]
pub struct Discovery {
    multicast_addr: SocketAddr,
    binding_addr: SocketAddr,
    message: Vec<u8>,
    peerlist: Arc<RwLock<Vec<Peer>>>,
    stopper: Arc<RwLock<bool>>,
}

impl Default for Discovery {
    fn default() -> Self {
        Self {
            multicast_addr: SocketAddr::from((DEFAULT_V4_MULTICAST_ADDRESS, DEFAULT_PORT_NUMBER)),
            binding_addr: SocketAddr::from((INADDR_ANY, DEFAULT_PORT_NUMBER)),
            message: b"discover".to_vec(),
            peerlist: Arc::new(RwLock::new(Vec::new())),
            stopper: Arc::new(RwLock::new(false)),
        }
    }
}

impl Discovery {
    pub fn discover< C: Fn(&mut Vec<Peer>) + std::marker::Send + 'static >(&self, callback: C) -> Result<(), Box<dyn std::error::Error>> {
        let Discovery {
            multicast_addr,
            binding_addr,
            message,
            peerlist,
            stopper,
        } = self.clone();

        // Setup stoppers
        let (broadcast_stopper, receiver_stopper) = (stopper.clone(), stopper);

        // Create the discovery socket.
        let send_socket = UdpSocket::bind(binding_addr).expect("Fuck");
        let receive_socket = send_socket
            .try_clone()
            .expect("Could not clone the socket!");

        // Start the broadcasting, pass the stopper.
        concurrent! {
            broadcast(send_socket, &multicast_addr, message,
            &broadcast_stopper).unwrap();
        };

        // Listen for broadcast.
        concurrent! {
            receiver(
                receive_socket,
                multicast_addr.ip(),
                binding_addr.ip(),
                &peerlist,
                callback,
                &receiver_stopper,
            ).unwrap();
        };
        Ok(())
    }

}

fn receiver<C: Fn(&mut Vec<Peer>) + std::marker::Send + 'static>(
    socket: UdpSocket,
    multicast_address: IpAddr,
    binding_addr: IpAddr,
    peerlist: &RwLock<Vec<Peer>>,
    callback: C,
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
        println!("Got packet");
        let peer = Peer {
            ip: src.ip(),
            data: buff[0..amount].to_vec(),
        };
        let mut peerlist_guard = peerlist.write().expect("Deadlock");
        if !peerlist_guard.contains(&peer) {
            peerlist_guard.push(peer);
            callback(&mut (*peerlist_guard).clone())
        }
    }
}

fn broadcast(
    socket: UdpSocket,
    multicast_address: &SocketAddr,
    message: Vec<u8>,
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
        socket.send_to(&message, multicast_address.to_string())?;
        std::thread::sleep(std::time::Duration::new(1, 0));
    }
}
