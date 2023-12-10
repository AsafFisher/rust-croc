use anyhow::{Context, Result};
use crypto::pake::PakePubKey;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::convert::TryFrom;

use super::CrocProto;

#[derive(Serialize, Deserialize, Debug)]
pub struct PakeMessage {
    #[serde(rename = "b")]
    pub(crate) bytes: Vec<u8>,
    #[serde(rename = "b2")]
    pub(crate) bytes2: Vec<u8>,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct ExternalIPMessage {
    #[serde(rename = "m")]
    pub(crate) external_ip: String,
}

impl PakeMessage {
    pub fn new(pub_key: &PakePubKey, curve_type: String) -> Result<Message> {
        Ok(Message::Pake(PakeMessage {
            bytes: serde_json::to_string(&pub_key)?.into(),
            bytes2: curve_type.into(),
        }))
    }
}
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "t")]
pub enum Message {
    #[serde(rename = "pake")]
    Pake(PakeMessage),
    #[serde(rename = "externalip")]
    ExternalIP(ExternalIPMessage),
    #[serde(rename = "finished")]
    Finished,
}
impl TryFrom<&[u8]> for Message {
    type Error = serde_json::Error;
    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        serde_json::from_slice(value)
    }
}
impl Message {
    pub async fn send(&self, conn: &mut CrocProto) -> Result<()>
    where
        Self: Serialize,
    {
        conn.write(serde_json::to_string(&self)?.as_bytes()).await
    }
    pub async fn recv(conn: &mut CrocProto) -> Result<Self>
    where
        Self: DeserializeOwned,
    {
        let test = conn.read().await?;
        let bytes_read = &String::from_utf8(test.clone())?;
        Ok(serde_json::from_str(bytes_read)
            .context(format!("Could not parse received data: {test:?}"))?)
    }

    // pub async fn process(self, client: &mut ClientSession) -> Result<Option<Message>> {
    //     match self {
    //         Message::Pake(msg) => msg.process(client).await,
    //         Message::ExternalIP(msg) => msg.process(client).await,
    //         Message::Finished => Ok(Message::Finished.into()),
    //     }
    // }
}
