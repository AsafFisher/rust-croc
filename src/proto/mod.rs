use anyhow::{anyhow, Context, Result};
use byteorder::LittleEndian;
mod croc_raw;
mod croc_enc;
pub use croc_enc::EncryptedSession;
pub use croc_raw::CrocProto;
use crypto::{
    aes::AesEncryptor,
    pake::{Pake, PakePubKey, Role},
};
use rand::RngCore;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    convert::{TryFrom, TryInto},
    io::{Read, Write},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, ToSocketAddrs},
};


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
