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
    join,
    net::tcp::{ReadHalf, WriteHalf},
    sync::Mutex,
    task::JoinHandle,
};

use crate::proto::{CrocProto, EncryptedSession};
use crypto::pake::Role;
struct Room {
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
    mut rooms: Arc<Mutex<HashMap<String, Room>>>,
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
        let mut guard = rooms.lock().await;
        if let Some(room) = guard.get_mut(&room_name) {
            if room.is_full() {
                let receiver = room.second.take().unwrap().connection;
                let sender = room.first.take().unwrap().connection;
                room.handle = relay(receiver, sender).await;
            } else {
                drop(guard);
                do_keepalive(rooms, room_name).await?
            }
        }
    }
    Ok(())
}
async fn do_keepalive(rooms: Arc<Mutex<HashMap<String, Room>>>, room_name: String) -> Result<()> {
    debug!("Starting keepalive");
    let mut should_delete_room = false;
    loop {
        let mut rooms = rooms.lock().await;
        if let Some(room) = rooms.get_mut(&room_name) {
            should_delete_room = if let Some(sender) = &mut room.first {
                debug!("Sending ping");
                match sender.write(&[1u8]).await {
                    Ok(_) => {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                    Err(err) => {
                        // If connection has some type of problem close the room
                        error!("Sender's socket stopped {}", err);
                        true
                    }
                }
            } else {
                // If we get here the ownership of room.first was taken by the relay task
                // and relay has started
                debug!("Room has started");
                false
            };
        }
        // No need for keepalive no more
        break;
    }
    if should_delete_room {
        let mut rooms = rooms.lock().await;
        rooms.remove(&room_name);
    }
    Ok(())
}
async fn relay(
    first: tokio::net::TcpStream,
    second: tokio::net::TcpStream,
) -> Option<JoinHandle<()>> {
    debug!("Relaying");
    Some(tokio::spawn(bridge_sockets(first, second)))
}
async fn negotiate_info(
    mut session: CrocProto,
    sym_key: &[u8; 32],
    relay_password: &str,
    multiplex_ports: Vec<u16>,
    rooms: &mut Arc<Mutex<HashMap<String, Room>>>,
) -> Result<Option<String>> {
    let mut enc = EncryptedSession::new(&mut session, sym_key, Role::Reciever).await?;
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
            if room.is_full() {
                debug!("Room is full");
                enc.write(&mut session, b"room full").await?;
                return Ok(None);
            } else {
                debug!("Adding receiver to {room_name}");
                enc.write(&mut session, b"ok").await?;
                room.second = Some(session);
                Ok(Some(room_name))
            }
        }
        None => {
            debug!("Creating room {room_name} and adding the sender to it");
            enc.write(&mut session, b"ok").await?;
            guard.insert(
                room_name.clone(),
                Room {
                    first: Some(session),
                    second: None,
                    opened: SystemTime::now().into(),
                    handle: None,
                },
            );
            Ok(Some(room_name))
        }
    }
}
#[derive(Clone)]
pub struct Relay<A: tokio::net::ToSocketAddrs> {
    rooms: Arc<Mutex<HashMap<String, Room>>>,
    bind_address: A,
    password: String,
    multiplex_ports: Vec<u16>,
}
async fn asymmetric_bridge_sockets<'a>(mut from: ReadHalf<'a>, mut to_s: WriteHalf<'a>) {
    loop {
        let mut buffer_a = [0u8; 1024];
        let amount = from.read(&mut buffer_a).await.unwrap();
        to_s.write_all(&mut buffer_a[..amount]).await.unwrap();
    }
}
async fn bridge_sockets(mut stream_a: tokio::net::TcpStream, mut stream_b: tokio::net::TcpStream) {
    // TODO: use tokio::io::copy
    // TODO: pass streams using channel
    let (a_r, a_w) = stream_a.split();
    let (b_r, b_w) = stream_b.split();
    let a_to_b = asymmetric_bridge_sockets(a_r, b_w);
    let b_to_a = asymmetric_bridge_sockets(b_r, a_w);
    let (_, _) = join!(a_to_b, b_to_a);
}

impl<A: tokio::net::ToSocketAddrs> Relay<A> {
    pub fn new(bind_address: A, password: String, multiplex_ports: Vec<u16>) -> Relay<A> {
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
        let listener = tokio::net::TcpListener::bind(&self.bind_address).await?;
        loop {
            let (stream, addr) = listener.accept().await?;
            debug!("Got client {addr}");
            tokio::spawn(handle(
                stream,
                self.password.clone(),
                self.multiplex_ports.clone(),
                self.rooms.clone(),
            ));
        }
    }
}
