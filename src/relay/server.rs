use anyhow::Result;
use chrono::{DateTime, Utc};
use std::{
    borrow::BorrowMut,
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::tcp::{ReadHalf, WriteHalf},
    sync::Mutex,
    task::JoinHandle,
    try_join,
};

use crate::proto::{AsyncCrocWrite, CrocProto, EncryptedSession};
use rust_pake::pake::Role;
pub struct Room {
    first: Option<CrocProto>,
    second: Option<CrocProto>,
    handle: Option<JoinHandle<()>>,
    opened: DateTime<Utc>,
}
impl Room {
    pub fn is_full(&self) -> bool {
        (self.first.is_some() && self.second.is_some()) || self.is_running()
    }
    pub fn is_running(&self) -> bool {
        self.handle
            .as_ref()
            .map_or(false, |handle| !handle.is_finished())
    }
    pub fn stop(&mut self) {
        self.handle.as_mut().map(|handle| handle.abort());
    }
}

async fn handle(
    client: tokio::net::TcpStream,
    relay_password: String,
    multiplex_ports: Vec<u16>,
    mut rooms: Arc<Mutex<HashMap<String, Arc<Mutex<Room>>>>>,
) -> Result<()> {
    let mut session = CrocProto::from_stream(client);
    let mut peeked_bytes = [0u8; 4];

    session.peek(&mut peeked_bytes).await?;
    if &peeked_bytes == b"ping" {
        debug!("Got ping");
        session.write(b"pong").await?;
        return Ok(());
    }
    let sym_key = session.negotiate_symmetric_key(Role::Reciever).await?;
    let room = negotiate_info(
        session,
        &sym_key,
        &relay_password,
        multiplex_ports,
        rooms.borrow_mut(),
    )
    .await?;
    if let Some(room_name) = room {
        let room = {
            let mut rooms = rooms.lock().await;
            rooms.get_mut(&room_name).map(|room| room.clone())
        };
        if let Some(room) = room {
            // We do not need to lock rooms anymore.
            let mut room_guard = room.lock().await;
            if room_guard.is_full() {
                let receiver = room_guard.second.take().unwrap().connection;
                let sender = room_guard.first.take().unwrap().connection;
                let result = relay(receiver, sender).await;
                room_guard.first = None;
                room_guard.second = None;
                let mut rooms = rooms.lock().await;
                rooms.remove(&room_name);
                debug!("RELAY ENDED: {result:?}");
            } else {
                drop(room_guard);
                // SAFTY: if a keepalive is sent, it will for sure
                // be sent before the relay start and will not be sent after
                // (because after the relay is establish it takes the room lock and never
                // releases it)
                do_keepalive(rooms, room_name).await?
            }
        }
    }
    Ok(())
}
async fn do_keepalive(
    rooms: Arc<Mutex<HashMap<String, Arc<Mutex<Room>>>>>,
    room_name: String,
) -> Result<()> {
    debug!("Starting keepalive");
    let room = {
        let mut rooms = rooms.lock().await;
        rooms.get_mut(&room_name).map(|room| room.clone())
    };
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if let Some(room) = &room {
            let mut room_guard = room.lock().await;
            if let Some(sender) = &mut room_guard.first {
                debug!("Sending ping");
                // Ping will never be sent after the `first` and `second` communicates.
                // On `croc`'s impl this byte can be sent by mistake after the relay starts.
                match sender.write(&[1u8]).await {
                    Ok(_) => {
                        continue;
                    }
                    Err(err) => {
                        // If connection has some type of problem close the room
                        error!("Sender's socket stopped {}", err);
                    }
                }
            } else {
                // If we get here the ownership of room.first was taken by the relay task
                // and relay has started
                debug!("Keepalive service terminating");
            };
        }
        // No need for keepalive no more
        break;
    }
    Ok(())
}
fn relay(first: tokio::net::TcpStream, second: tokio::net::TcpStream) -> JoinHandle<Result<()>> {
    debug!("Relaying");
    tokio::spawn(bridge_sockets(first, second))
}
async fn negotiate_info(
    mut session: CrocProto,
    sym_key: &[u8; 32],
    relay_password: &str,
    multiplex_ports: Vec<u16>,
    rooms: &mut Arc<Mutex<HashMap<String, Arc<Mutex<Room>>>>>,
) -> Result<Option<String>> {
    let enc = EncryptedSession::new(&mut session, sym_key, Role::Reciever).await?;
    let password = String::from_utf8(enc.read(&mut session).await?)?;
    if password != relay_password.trim() {
        debug!("Bad password {password}");
        enc.write(&mut session, b"bad password").await?
    }
    let message = if multiplex_ports.len() == 0 {
        "ok".to_string()
    } else {
        multiplex_ports
            .iter()
            .map(|port| port.to_string())
            .collect::<Vec<String>>()
            .join(",")
    } + "|||"
        + &(session.connection.peer_addr()?.to_string());

    enc.write(&mut session, message.as_bytes()).await?;

    let room_name = String::from_utf8(enc.read(&mut session).await?)?;
    let mut guard = rooms.lock().await;
    match guard.get_mut(&room_name) {
        Some(room) => {
            let mut room_guard = room.lock().await;
            if room_guard.is_full() {
                debug!("Room is full");
                enc.write(&mut session, b"room full").await?;
                return Ok(None);
            } else {
                debug!("Adding receiver to {room_name}");
                enc.write(&mut session, b"ok").await?;
                room_guard.second = Some(session);
                Ok(Some(room_name))
            }
        }
        None => {
            debug!("Creating room {room_name} and adding the sender to it");
            enc.write(&mut session, b"ok").await?;
            guard.insert(
                room_name.clone(),
                Arc::new(Mutex::new(Room {
                    first: Some(session),
                    second: None,
                    opened: SystemTime::now().into(),
                    handle: None,
                })),
            );
            Ok(Some(room_name))
        }
    }
}
#[derive(Clone)]
pub struct Relay {
    rooms: Arc<Mutex<HashMap<String, Arc<Mutex<Room>>>>>,
    bind_address: String,
    password: String,
    multiplex_ports: Vec<u16>,
}

// TODO: Add error handling
async fn asymmetric_bridge_sockets<'a>(
    mut from: ReadHalf<'a>,
    mut to_s: WriteHalf<'a>,
) -> Result<()> {
    loop {
        let mut buffer_a = [0u8; 1024];
        let amount = from.read(&mut buffer_a).await?;
        if amount == 0 {
            return Ok(());
        }
        to_s.write_all(&mut buffer_a[..amount]).await?;
    }
}
async fn bridge_sockets(
    mut stream_a: tokio::net::TcpStream,
    mut stream_b: tokio::net::TcpStream,
) -> Result<()> {
    // TODO: use tokio::io::copy
    // TODO: pass streams using channel
    let (a_r, a_w) = stream_a.split();
    let (b_r, b_w) = stream_b.split();
    let a_to_b = asymmetric_bridge_sockets(a_r, b_w);
    let b_to_a = asymmetric_bridge_sockets(b_r, a_w);
    try_join!(a_to_b, b_to_a)?;
    Ok(())
}

pub async fn run_instance(
    password: String,
    multiplex_ports: Vec<u16>,
    rooms: Arc<Mutex<HashMap<String, Arc<Mutex<Room>>>>>,
    bind_address: std::net::SocketAddr,
) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    loop {
        let (stream, addr) = listener.accept().await?;
        debug!("Got client {addr}");
        tokio::spawn(handle(
            stream,
            password.clone(),
            multiplex_ports.clone(),
            rooms.clone(),
        ));
    }
}

impl Relay {
    pub fn new(bind_address: String, password: String, multiplex_ports: Vec<u16>) -> Relay {
        Relay {
            rooms: Arc::new(Mutex::new(HashMap::new())),
            bind_address,
            password,
            multiplex_ports,
        }
    }
    pub async fn start(self) -> Result<()> {
        // TODO: Create a delete old room task
        debug!("Starting relay");
        debug!("Creating file relay sockets");
        let bind_ip = self.bind_address.parse::<std::net::SocketAddr>()?.ip();
        for address in self
            .multiplex_ports
            .iter()
            .map(|port| std::net::SocketAddr::new(bind_ip, *port))
        {
            tokio::spawn(run_instance(
                "pass123".to_string(),
                self.multiplex_ports.clone(),
                self.rooms.clone(),
                address,
            ));
        }

        debug!("Creating relay socket");
        run_instance(
            self.password,
            self.multiplex_ports,
            self.rooms,
            self.bind_address.parse()?,
        )
        .await
    }
}
