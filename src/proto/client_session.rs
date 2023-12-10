use std::{convert::TryInto, marker::PhantomData, ops::DerefMut};

use aes_gcm::Key;
use anyhow::{Result, anyhow};
use crypto::{
    aes::AesEncryptor,
    pake::{Pake, Role},
};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::proto::{croc_msg::ExternalIPMessage, croc_raw::ProtoError};

use super::{
    croc_msg::{Message, PakeMessage},
    CrocProto, EncryptedSession,
};
#[derive(Serialize, Deserialize)]
struct FileInfo {
    #[serde(rename = "Name")]
    name: String,
}
#[derive(Default, Debug, PartialEq)]
enum ClientState {
    #[default]
    KeyExchange,
    IpExchange,
    FileInfoTransfare,
    FileTransfare,
}
trait Process<T> {
    async fn process(self, msg: Message) -> Result<T>;
}

pub struct ClientSession {
    state: ClientState,
    pub stream: CrocProto,
    encrypted_session: Option<EncryptedSession>,
    shared_secret: String,
    pub is_sender: bool,
    external_ip: String,
    peer_external_ip: Option<String>,
    key: Option<Pake<crypto::pake::SIEC255Params>>,
}

impl ClientSession {
    pub fn new(
        stream: CrocProto,
        shared_secret: String,
        is_sender: bool,
        external_ip: String,
    ) -> Self {
        Self {
            state: ClientState::KeyExchange,
            stream,
            encrypted_session: None,
            shared_secret,
            is_sender,
            external_ip,
            peer_external_ip: None,
            key: None,
        }
    }
    pub async fn process_client(mut self) -> Result<()> {
        if !self.is_sender {
            debug!("Receiver Started: Sending initial key");
            self.key = Some(Pake::new(
                Role::Sender,
                self.shared_secret[5..].as_bytes().into(),
            ));
            PakeMessage::new(&self.key.as_ref().unwrap().pub_pake, "siec".to_string())?
                .send(&mut self.stream)
                .await?;
        }else{
            debug!("Sender Started: Should get key req");
        }
        loop {
            debug!("Waiting for message");
            self.step()?;
            let msg = Message::recv(&mut self.stream).await?;
            debug!("Got Message");
            self.step()?;
            match msg {
                Message::Pake(msg) => self.process_key_exchange(msg).await?,
                Message::ExternalIP(msg) => self.process_ip_exchange(msg).await?,
                Message::Finished => return Ok(()),
            }
        }
    }
    // pub async fn process(self, msg: Message) -> Result<Option<Message>> {
    //     match self {
    //         Message::Pake(msg) => msg.process(client).await,
    //         Message::ExternalIP(msg) => msg.process(client).await,
    //         Message::Finished => Ok(Message::Finished.into()),
    //     }
    // }
    fn step(&self) -> Result<()> {
        debug!(
            "{}: State - {:?}",
            if self.is_sender { "Sender" } else { "Reciever" },
            self.state
        );
        Ok(())
    }
}

impl ClientSession {
    async fn process_key_exchange(&mut self, msg: PakeMessage) -> Result<()> {
        let mut salt = [0u8; 8];
        if self.is_sender {
            if msg.bytes2 != b"siec" {
                error!("Protocol not supported");
                return Err(ProtoError::CurveNotSupported.into());
            }
            let mut key = Pake::new(Role::Reciever, Some(self.shared_secret[5..].as_bytes()));
            key.update(serde_json::from_str(&String::from_utf8(
                msg.bytes.clone(),
            )?)?)?;

            let mut rnd = rand::thread_rng();
            rnd.fill_bytes(&mut salt);
            let msg = Message::Pake(PakeMessage {
                bytes: serde_json::to_string(&key.pub_pake)?.as_bytes().to_vec(),
                bytes2: salt.to_vec(),
            });
            self.key = Some(key);
            debug!("Senging to Receiver");
            self.stream
                .write(serde_json::to_string(&msg)?.as_bytes())
                .await?;
        } else {
            if let Some(key) = &mut self.key {
                key.update(serde_json::from_slice(msg.bytes.as_slice())?)?;
                salt = msg.bytes2.as_slice().try_into()?;
            } else {
                return Err(ProtoError::CurveNotInitialized.into());
            }
        }
        let key = self.key.as_ref().unwrap().k.unwrap();
        self.encrypted_session = Some(EncryptedSession::from_encryptor(AesEncryptor::new(
            &key,
            Some(salt),
        )));
        // Should Connect to other relay ports
        //====================================
        // ...
        if !self.is_sender {
            debug!("Receiver Sending IP");
            Message::ExternalIP(ExternalIPMessage {
                external_ip: self.external_ip.clone(),
            })
            .send(&mut self.stream)
            .await?;
        }
        self.state = ClientState::IpExchange;
        Ok(())
        // Usually connects
    }
}

impl ClientSession {
    async fn process_ip_exchange(&mut self, msg: ExternalIPMessage) -> Result<()> {
        if self.is_sender {
            Message::ExternalIP(ExternalIPMessage {
                external_ip: self.external_ip.clone(),
            })
            .send(&mut self.stream)
            .await?
        }
        self.peer_external_ip = Some(msg.external_ip);
        debug!(
            "connected as {} -> {}",
            self.external_ip,
            self.peer_external_ip.as_ref().unwrap()
        );
        self.state = ClientState::FileInfoTransfare;
        Ok(())
    }
}
