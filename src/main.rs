//mod cli;
extern crate pretty_env_logger;
#[macro_use]
extern crate log;

mod proto;
mod relay;
use anyhow::{Context, Result};
use crypto::pake::Role;
use proto::{CrocProto, EncryptedSession};
use std::{path::PathBuf, vec};
use tokio::net::ToSocketAddrs;

use crate::proto::client_session::ClientSession;

#[derive(thiserror::Error, Debug)]
enum RelayClientError {
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
    #[error("Shared secret used in client is invalid {0}")]
    BadSharedSecret(String),
}
struct Config {
    relay_ports: Vec<String>,
    relay_address: String,
    relay_password: String,
    room: String,
    disable_local: bool,
}

struct RelayClient {
    stream: CrocProto,
    files: Vec<PathBuf>,
    relay_ports: Vec<String>,
    external_ip: Option<String>,
    disable_local: bool,
    is_sender: bool,
    shared_secret: String,
}
impl RelayClient {
    pub async fn connect<A: ToSocketAddrs>(
        relay_addr: A,
        password: &str,
        shared_secret: &str,
        disable_local: bool,
        is_sender: bool,
    ) -> Result<Self> {
        if shared_secret.len() < 4 {
            return Err(RelayClientError::BadSharedSecret(shared_secret.to_string()).into());
        }
        let mut transferer = RelayClient {
            stream: CrocProto::connect(relay_addr).await?,
            files: vec![],
            relay_ports: vec![],
            disable_local,
            is_sender,
            shared_secret: shared_secret.to_string(),
            external_ip: None,
        };
        let sym_key = &transferer
            .stream
            .negotiate_symmetric_key(crypto::pake::Role::Sender)
            .await?;
        transferer
            .negotiate_info(sym_key, password, &shared_secret[..3])
            .await?;
        Ok(transferer)
    }

    async fn connect_to_sender(mut self) -> Result<ClientSession> {
        debug!("Sending handshake");
        // Keep the connection untill a transfer request has
        self.stream.write(b"handshake").await?;
        Ok(ClientSession::new(
            self.stream,
            self.shared_secret,
            self.is_sender,
            self.external_ip.context("Did not receive external IP")?,
        ))
    }
    async fn wait_for_receiver(mut self) -> Result<ClientSession> {
        // Keep the connection untill a transfer request has
        self.handle_keepalive().await?;
        Ok(ClientSession::new(
            self.stream,
            self.shared_secret,
            self.is_sender,
            self.external_ip.context("Did not receive external IP")?,
        ))
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
                _ => return Err(RelayClientError::UnknownKeepaliveMessage(data))?,
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
        enc.write(&mut self.stream, password.as_bytes()).await?;

        // Banner/IpAddress
        let message = String::from_utf8(enc.read(&mut self.stream).await?)?;
        if !message.contains("|||") {
            return Err(RelayClientError::BadResponse(message.to_string()))?;
        }
        let info: Vec<&str> = message.split("|||").collect();
        let banner = info[0];
        let ipaddr = info[1];
        self.external_ip = Some(ipaddr.to_string());
        debug!("Benner: {banner}");
        debug!("Ipaddr: {ipaddr}");
        self.relay_ports = banner.split(",").map(|banner| banner.to_string()).collect();

        debug!("Negotiating room: {room}");
        // Send room number
        enc.write(&mut self.stream, room.as_bytes()).await?;

        let response = enc.read(&mut self.stream).await?;
        if response != b"ok" {
            return if response == b"room full" {
                Err(RelayClientError::RoomFull(room.to_string()))?
            } else {
                Err(RelayClientError::RoomNegotiationFailed)?
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

    async fn sender() -> Result<()> {
        let default_relay_addr = "localhost:9009";
        let transferer =
            RelayClient::connect(default_relay_addr, "pass123", "12345", false, true).await?;
        let mut client = transferer.wait_for_receiver().await?;
        debug!("Start sanding");
        let a = client.process_client().await;
        debug!("Returned");
        a?;
        Ok(())
    }
    async fn receiver() -> Result<()> {
        let default_relay_addr = "localhost:9009";
        let transferer2 =
            RelayClient::connect(default_relay_addr, "pass123", "12345", false, false).await?;
        let mut client2 = transferer2.connect_to_sender().await?;
        debug!("Start receiving");
        client2.process_client().await?;
        Ok(())
    }
    let (res1, res2, res3) = tokio::join!(relay_task, sender(), receiver());

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
