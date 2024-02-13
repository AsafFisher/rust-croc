use crate::crypto::aes::AesEncryptor;
use anyhow::Result;
use rust_pake::pake::Role;
use std::convert::TryInto;

use super::{
    croc_raw::{AsyncCrocRead, AsyncCrocWrite},
    CrocProto,
};

#[derive(Clone)]
pub struct EncryptedSession {
    encryptor: AesEncryptor,
}

impl EncryptedSession {
    pub async fn new(
        session: &mut CrocProto,
        session_key: &[u8; 32],
        role: Role,
    ) -> Result<EncryptedSession> {
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
            encryptor: encryptor,
        })
    }
    pub fn from_encryptor(encryptor: AesEncryptor) -> EncryptedSession {
        Self { encryptor }
    }
    pub fn as_encryptor(&self) -> &AesEncryptor {
        &self.encryptor
    }
    pub async fn write<S: AsyncCrocWrite>(&self, session: &mut S, msg: &[u8]) -> Result<()> {
        let encrypted_data = self.encryptor.encrypt(msg)?;
        session.write(&encrypted_data).await
    }
    pub async fn read<S: AsyncCrocRead>(&self, session: &mut S) -> Result<Vec<u8>> {
        self.encryptor.decrypt(&session.read().await?)
    }
}
