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

#[derive(Debug)]
pub struct Peer {
    ip: IpAddr,
    data: Vec<u8>,
}

pub fn discover() -> Result<Vec<Peer>, Box<dyn std::error::Error>> {
    let multicast_address = DEFAULT_V4_MULTICAST_ADDRESS;
    let port = DEFAULT_PORT_NUMBER;

    let multicast_addr = SocketAddr::from((multicast_address, port));
    let binding_addr = SocketAddr::from((INADDR_ANY, DEFAULT_PORT_NUMBER));

    let mut peerlist = Vec::new();
    let stopper = Arc::new(RwLock::new(false));

    // Create the discovery socket.
    let receive_socket = UdpSocket::bind(binding_addr).expect("Fuck");

    let send_socket = receive_socket
        .try_clone()
        .expect("Could not clone the socket!");
    let b_stopper = stopper.clone();

    // Start the broadcasting, pass the stopper.
    let handle = concurrent! {
        broadcast(send_socket, multicast_addr,
        &b_stopper).unwrap();
    };

    // Listen for broadcast.
    listen(
        receive_socket,
        multicast_addr.ip(),
        binding_addr.ip(),
        &mut peerlist,
    )?;

    // Signal the broadcasting process to die.
    {
        let mut w_stop = stopper.write().expect("Deadlock");
        *w_stop = true;
    }

    // Wait for the process.
    handle.join().expect("Broadcaster paniced");

    Ok(peerlist)
}

fn listen(
    socket: UdpSocket,
    multicast_address: IpAddr,
    binding_addr: IpAddr,
    peerlist: &mut Vec<Peer>,
) -> Result<(), io::Error> {
    match (multicast_address, binding_addr) {
        (IpAddr::V4(mip), IpAddr::V4(bip)) => socket.join_multicast_v4(&mip, &bip),
        (IpAddr::V6(mip), _) => socket.join_multicast_v6(&mip, 0), // How do i build it good...
        _ => panic!("Can't combine ipv4 and ipv6."),
    }?;
    let mut buff = [0u8; 65535];

    let (amount, src) = socket.recv_from(&mut buff).unwrap();
    peerlist.push(Peer {
        ip: src.ip(),
        data: buff[0..amount].to_vec(),
    });
    Ok(())
}
fn broadcast(
    socket: UdpSocket,
    multicast_address: SocketAddr,
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
