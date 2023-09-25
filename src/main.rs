//mod cli;
extern crate pretty_env_logger;
#[macro_use]
extern crate log;

mod proto;
mod relay;
use anyhow::Result;
use crypto::pake::Role;
use proto::{CrocProto, EncryptedSession};
use std::{path::PathBuf, vec};
use tokio::net::ToSocketAddrs;

#[derive(thiserror::Error, Debug)]
enum TransferError {
    #[error("Something went wrong with received response {0}")]
    BadResponse(String),
    #[error("Symmetric Key negotiation failed")]
    KeyNegotiationFailiure,
    #[error("The room requested ({0}) is full")]
    RoomFull(String),
    #[error("Room negotiation failed for unknown reason")]
    RoomNegotiationFailed,
    #[error("Got unknown bytes from relay while keepaliving {0:?}")]
    UnknownKeepaliveMessage(Vec<u8>),
}
struct Config {
    relay_ports: Vec<String>,
    relay_address: String,
    relay_password: String,
    room: String,
    disable_local: bool,
}

struct FilesTransfer {
    stream: CrocProto,
    files: Vec<PathBuf>,
    relay_ports: Vec<String>,
    disable_local: bool,
}
impl FilesTransfer {
    pub async fn connect<A: ToSocketAddrs>(
        relay_addr: A,
        password: &str,
        room: &str,
        disable_local: bool,
    ) -> Result<Self> {
        let mut transferer = FilesTransfer {
            stream: CrocProto::connect(relay_addr).await?,
            files: vec![],
            relay_ports: vec![],
            disable_local,
        };
        let sym_key = &transferer
            .stream
            .negotiate_symmetric_key(crypto::pake::Role::Sender)
            .await?;
        transferer.negotiate_info(sym_key, password, room).await?;
        Ok(transferer)
    }
    pub async fn send(&mut self) -> Result<()> {
        // Keep the connection untill a transfer request has
        self.handle_keepalive().await?;
        // Can be async
        self.process_relay()?;
        self.transfer()?;
        Ok(())
    }
    pub async fn recv(&mut self) -> Result<()> {
        debug!("Sending handshake");
        // Keep the connection untill a transfer request has
        self.stream.write(b"handshake").await?;

        // Can be async
        self.process_relay()?;
        self.transfer()?;
        Ok(())
    }
    pub fn path(mut self, path: PathBuf) -> Self {
        self.files.push(path);
        self
    }
    pub fn paths(&mut self, mut paths: Vec<PathBuf>) {
        self.files.append(&mut paths);
    }
    pub async fn handle_keepalive(&mut self) -> Result<()> {
        info!(
            "Starting keepalive {}",
            self.stream.connection.local_addr()?
        );
        loop {
            let data = self.stream.read().await?;
            match data.as_slice() {
                b"ips?" => {
                    let mut ips = vec![];
                    if !self.disable_local {
                        ips.push(self.relay_ports[0].clone());
                        let interfaces = default_net::get_interfaces();
                        for interface in interfaces {
                            for ip in interface.ipv4 {
                                if ip.addr.is_loopback() {
                                    continue;
                                }
                                ips.push(ip.addr.to_string())
                            }
                        }
                    }
                    let outbips = serde_json::to_string(&ips)?;
                    debug!("Sending Ips: {outbips}");
                    self.stream.write(outbips.as_bytes()).await?
                }
                b"handshake" => {
                    return {
                        info!("Got handshake");
                        Ok(())
                    }
                }
                [1u8] => {
                    // Ping
                    debug!("Got ping");
                }
                _ => return Err(TransferError::UnknownKeepaliveMessage(data))?,
            }
        }
    }
    fn process_relay(&self) -> Result<()> {
        todo!()
    }
    fn transfer(&self) -> Result<()> {
        todo!()
    }

    async fn negotiate_info(
        &mut self,
        sym_key: &[u8; 32],
        password: &str,
        room: &str,
    ) -> Result<()> {
        let mut enc = EncryptedSession::new(&mut self.stream, sym_key, Role::Sender).await?;

        // Transfare password
        enc.write(password.as_bytes()).await?;

        // Banner/IpAddress
        let message = String::from_utf8(enc.read().await?)?;
        if !message.contains("|||") {
            return Err(TransferError::BadResponse(message.to_string()))?;
        }
        let info: Vec<&str> = message.split("|||").collect();
        let banner = info[0];
        let ipaddr = info[1];
        debug!("Benner: {banner}");
        debug!("Ipaddr: {ipaddr}");
        self.relay_ports = banner.split(",").map(|banner| banner.to_string()).collect();

        debug!("Negotiating room: {room}");
        // Send room number
        enc.write(room.as_bytes()).await?;

        let response = enc.read().await?;
        if response != b"ok" {
            return if response == b"room full" {
                Err(TransferError::RoomFull(room.to_string()))?
            } else {
                Err(TransferError::RoomNegotiationFailed)?
            };
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();
    let relay_task = tokio::task::spawn(async {
        let relay = relay::Relay::new("0.0.0.0:9009", "pass123".to_string(), vec![9010, 9110]);
        relay.start().await.unwrap();
    });

    let default_relay_addr = "localhost:9009";
    let mut transferer =
        FilesTransfer::connect(default_relay_addr, "pass123", "123", false).await?;
    let client = transferer.send();

    let default_relay_addr = "localhost:9009";
    let mut transferer2 =
        FilesTransfer::connect(default_relay_addr, "pass123", "123", false).await?;
    let client2 = transferer2.recv();

    let (_res, _re2, _res3) = tokio::join!(relay_task, client, client2);

    //println!("{}", transferer.code);
    // let client = proto::CrocProto::from_stream(a);

    // let mut buff = [0u8, 4];
    // //let _ = a.read(&mut buff);
    // println!("{:?}", buff);

    // println!("Success!");
    Ok(())
    // match cli::run(){
    //     Ok(()) => Ok(()),
    //     Err(err) => panic!("{:?}", err)
    // }
}
