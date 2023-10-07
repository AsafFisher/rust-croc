use anyhow::{anyhow, Context, Result};
use byteorder::LittleEndian;
use crypto::{
    aes::AesEncryptor,
    pake::{Pake, PakePubKey, Role},
};
use serde::{Deserialize, Serialize};
use std::{
    convert::{TryFrom, TryInto},
    io::{Read, Write},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, ToSocketAddrs},
};

#[derive(thiserror::Error, Debug)]
pub enum ProtoError {
    #[error("Symmetric Key negotiation failed")]
    KeyNegotiationFailiure,
}
const CROC_MAGIC: &[u8; 4] = b"croc";

pub struct CrocProto {
    pub connection: TcpStream,
}
// impl Clone for CrocProto {
//     fn clone(&self) -> Self {
//         Self { connection: TcpStream::from_std(self.connection.into_std().unwrap().try_clone().unwrap()).unwrap() }
//     }

//     fn clone_from(&mut self, source: &Self) {
//         *self = source.clone()
//     }
// }
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

pub struct EncryptedSession<'a> {
    session: &'a mut CrocProto,
    encryptor: AesEncryptor,
}

impl<'a> EncryptedSession<'a> {
    pub async fn new(
        session: &'a mut CrocProto,
        session_key: &[u8; 32],
        role: Role,
    ) -> Result<EncryptedSession<'a>> {
        let encryptor = match role {
            Role::Sender => {
                let encryptor = AesEncryptor::new(session_key, None);
                // Let the server know the salt
                session.write(&encryptor.salt).await?;
                encryptor
            }
            Role::Reciever => {
                let salt = session.read().await?;
                AesEncryptor::new(session_key, Some(salt.as_slice().try_into()?))
            }
        };
        Ok(Self {
            session: session,
            encryptor: encryptor,
        })
    }
    pub async fn write(&mut self, msg: &[u8]) -> Result<()> {
        let encrypted_data = self.encryptor.encrypt(msg)?;
        self.session.write(&encrypted_data).await
    }
    pub async fn read(&mut self) -> Result<Vec<u8>> {
        self.encryptor.decrypt(&self.session.read().await?)
    }
}
impl<'a> std::ops::Deref for EncryptedSession<'a> {
    type Target = TcpStream;

    fn deref(&self) -> &Self::Target {
        &self.session.connection
    }
}

pub trait CrocMessage {
    fn process(&self) -> Result<Message>;
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PakeMessage {
    #[serde(rename = "b")]
    bytes: String,
    #[serde(rename = "b2")]
    bytes2: String,
}
impl CrocMessage for PakeMessage {
    fn process(&self) -> Result<Message> {
        todo!()
    }
}
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "t")]
pub enum Message {
    #[serde(rename = "pake")]
    Pake(PakeMessage),
    #[serde(rename = "finished")]
    Finished,
}
impl TryFrom<&[u8]> for Message {
    type Error = serde_json::Error;
    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        serde_json::from_slice(value)
    }
}

fn process_message(message: &[u8]) -> Result<Message> {
    let msg: Message = message.try_into().unwrap();
    // process_message
    match msg {
        Message::Pake(msg) => msg.process(),
        Message::Finished => Ok(Message::Finished),
    }
}

#[cfg(test)]
mod test {
    use std::net::TcpStream;

    use crypto::pake::{Pake, Role};

    use super::{process_message, CrocProto};

    #[test]
    fn test_ping_proto() {
        // let default_relay_addr = "croc.schollz.com:9009";
        // {
        //     let _pake = Pake::new(Role::Sender);

        //     let mut proto = CrocProto::connect(default_relay_addr).unwrap();
        //     proto.write(b"ping").unwrap();
        //     assert_eq!(String::from_utf8_lossy(&proto.read().unwrap()), "pong")
        // }
        // {
        //     let _pake = Pake::new(Role::Sender);
        //     let stream = TcpStream::connect(default_relay_addr).unwrap();
        //     let mut proto = CrocProto::from_stream(stream);
        //     proto.write(b"ping").unwrap();
        //     assert_eq!(String::from_utf8_lossy(&proto.read().unwrap()), "pong")
        // }
    }

    #[test]
    fn test_process_message() {
        let _hello =
            process_message("{\"t\": \"pake\", \"b\": \"hello\", \"b2\":\"hello\"}".as_bytes())
                .unwrap();
    }
}
