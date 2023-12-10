use anyhow::{anyhow, Context, Result};
use byteorder::LittleEndian;
use crypto::pake::{Pake, PakePubKey, Role};
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
    pub async fn write(&mut self, msg: &[u8]) -> Result<()> {
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
    pub async fn read(&mut self) -> Result<Vec<u8>> {
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
