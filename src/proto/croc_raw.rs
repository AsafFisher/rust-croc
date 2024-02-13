use anyhow::{anyhow, Context, Result};
use byteorder::LittleEndian;
use rust_pake::pake::{Pake, PakePubKey, Role};
use std::convert::TryInto;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, ToSocketAddrs},
};



#[derive(thiserror::Error, Debug)]
pub enum ProtoError {
    #[error("Symmetric Key negotiation failed")]
    KeyNegotiationFailiure,
    #[error("Curve received is not supported")]
    CurveNotSupported,
    #[error("Curve was not initialized")]
    CurveNotInitialized,
}
const CROC_MAGIC: &[u8; 4] = b"croc";
pub trait AsyncCrocRead {
    async fn read(&mut self) -> Result<Vec<u8>>;
}
pub trait AsyncCrocWrite {
    async fn write(&mut self, msg: &[u8]) -> Result<()>;
}

pub struct OwnedReceiver {
    pub receiver: tokio::sync::mpsc::Receiver<Vec<u8>>,
}
impl AsyncCrocRead for OwnedReceiver {
    async fn read(&mut self) -> Result<Vec<u8>> {
        self.receiver.recv().await.ok_or(anyhow!("Channel closed"))
    }
}

#[derive(Clone)]
pub struct OwnedSender {
    pub sender: tokio::sync::mpsc::Sender<Vec<u8>>,
}
impl AsyncCrocWrite for OwnedSender {
    async fn write(&mut self, msg: &[u8]) -> Result<()> {
        let mut buffer = vec![];
        std::io::Write::write_all(&mut buffer, CROC_MAGIC)?;
        byteorder::WriteBytesExt::write_u32::<LittleEndian>(&mut buffer, msg.len().try_into()?)?;
        std::io::Write::write_all(&mut buffer, &msg)?;
        self.sender.send(buffer.to_vec()).await?;
        Ok(())
    }
}
pub struct MpscCrocProto {
    pub receiver: tokio::sync::mpsc::Receiver<Vec<u8>>,
    pub sender: tokio::sync::mpsc::Sender<Vec<u8>>,
}

impl MpscCrocProto {
    // we dont use the logic of CrocProto within the tasks because it is CPU intensive and we dont want to block the read and write tasks:
    // new function will create two tasks:
    // 1. that will read from the socket and put it in the channel
    // 2. that will read from the channel and write to the socket
    pub fn from_stream(connection: TcpStream) -> Result<Self> {
        let (mut read, mut write) = connection.into_split();
        let (write_sender, mut write_receiver) = tokio::sync::mpsc::channel::<Vec<u8>>(100);
        let (read_sender, read_receiver) = tokio::sync::mpsc::channel(100);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(msg) = write_receiver.recv() => {
                        if let Err(e) = write.write_all(&msg).await {
                            error!("Error writing to socket: {}", e);
                            break;
                        }
                    }
                    else => {
                        break;
                    }
                }
            }
        });
        tokio::spawn(async move {
            loop {
                let mut header_magic = [0u8; 4];
                if let Err(e) = read.read_exact(&mut header_magic).await {
                    error!("Error reading from socket: {}", e);
                    break;
                }
                if &header_magic != CROC_MAGIC {
                    error!("Bad magic {:?}", header_magic);
                    break;
                }
                // read message len
                let msg_len = if let Ok(msg_len) = read.read_u32_le().await {
                    msg_len
                } else {
                    error!("Error reading message len");
                    break;
                };
                let mut message = vec![0u8; msg_len.try_into().unwrap()];
                if let Err(e) = read.read_exact(message.as_mut_slice()).await {
                    error!("Error reading from socket: {}", e);
                    break;
                }
                // Hack, to fix a bug with `croc`'s original relay where a pint (1u8) is sent by the server
                // after the connection was established between the two parties.
                if message.len() == 1 && message[0] == 1u8 {
                    trace!("Received 1u8, ignoring");
                    continue;
                }
                // Pass the message to the channel
                if let Err(e) = read_sender.send(message).await {
                    error!("Error sending to channel: {}", e);
                    break;
                }
            }
        });
        Ok(MpscCrocProto {
            receiver: read_receiver,
            sender: write_sender,
        })
    }
    pub fn into_split(self) -> (OwnedReceiver, OwnedSender) {
        (
            OwnedReceiver {
                receiver: self.receiver,
            },
            OwnedSender {
                sender: self.sender,
            },
        )
    }
}
impl AsyncCrocRead for MpscCrocProto {
    async fn read(&mut self) -> Result<Vec<u8>> {
        self.receiver.recv().await.ok_or(anyhow!("Channel closed"))
    }
}
impl AsyncCrocWrite for MpscCrocProto {
    async fn write(&mut self, msg: &[u8]) -> Result<()> {
        let mut buffer = vec![];
        std::io::Write::write_all(&mut buffer, CROC_MAGIC)?;
        byteorder::WriteBytesExt::write_u32::<LittleEndian>(&mut buffer, msg.len().try_into()?)?;
        std::io::Write::write_all(&mut buffer, &msg)?;
        self.sender.send(buffer.to_vec()).await?;
        Ok(())
    }
}
pub struct CrocProto {
    pub connection: TcpStream,
}

impl CrocProto {
    #![allow(dead_code)]
    pub fn from_stream(connection: TcpStream) -> Self {
        CrocProto {
            connection: connection,
        }
    }
    pub async fn connect<A: ToSocketAddrs>(addr: A) -> Result<Self> {
        Ok(CrocProto {
            connection: TcpStream::connect(addr).await?,
        })
    }
    pub async fn peek(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.connection.peek(buf).await
    }

    /// Uses assymetric eliptic curve to match a symmetric key
    pub async fn negotiate_symmetric_key(&mut self, role: Role) -> Result<[u8; 32]> {
        let key = Pake::new(role, None);
        match role {
            Role::Sender => {
                let mut a_key = key;
                debug!(
                    "sender a_key: {}",
                    serde_json::to_string_pretty(&a_key.pub_pake).unwrap()
                );
                self.write(serde_json::to_string(&a_key.pub_pake)?.as_bytes())
                    .await?;

                let b_key: PakePubKey =
                    serde_json::from_str(std::str::from_utf8(self.read().await?.as_slice())?)?;
                debug!(
                    "sender b_key: {}",
                    serde_json::to_string_pretty(&b_key).unwrap()
                );
                a_key.update(b_key)?;

                // strong key - this is our symetric key
                a_key.k.ok_or(ProtoError::KeyNegotiationFailiure.into())
            }
            Role::Reciever => {
                let mut b_key = key;
                debug!(
                    "reciever b_key: {}",
                    serde_json::to_string_pretty(&b_key.pub_pake).unwrap()
                );
                let a_key: PakePubKey =
                    serde_json::from_str(std::str::from_utf8(self.read().await?.as_slice())?)?;
                debug!(
                    "reciever a_key: {}",
                    serde_json::to_string_pretty(&a_key).unwrap()
                );
                b_key.update(a_key)?;
                self.write(serde_json::to_string(&b_key.pub_pake)?.as_bytes())
                    .await?;
                b_key.k.ok_or(ProtoError::KeyNegotiationFailiure.into())
            }
        }
    }
}

impl AsyncCrocRead for CrocProto {
    async fn read(&mut self) -> Result<Vec<u8>> {
        let mut header_magic = [0u8; 4];
        self.connection
            .read_exact(&mut header_magic)
            .await
            .context("Could not read magic")?;
        if &header_magic != CROC_MAGIC {
            return Err(anyhow!("Bad magic {:?}", header_magic));
        }
        let msg_len = self
            .connection
            .read_u32_le()
            .await
            .context("Could not read message size")?;
        let mut message = vec![0u8; msg_len.try_into().unwrap()];
        self.connection.read_exact(message.as_mut_slice()).await?;
        Ok(message)
    }
}
impl AsyncCrocWrite for CrocProto {
    async fn write(&mut self, msg: &[u8]) -> Result<()> {
        let mut buffer = vec![];
        std::io::Write::write_all(&mut buffer, CROC_MAGIC)?;
        byteorder::WriteBytesExt::write_u32::<LittleEndian>(&mut buffer, msg.len().try_into()?)?;
        std::io::Write::write_all(&mut buffer, &msg)?;
        self.connection
            .write_all(&buffer)
            .await
            .context("Could not send message")?;
        Ok(())
    }
}
