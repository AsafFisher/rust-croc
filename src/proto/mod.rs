use anyhow::{anyhow, Context, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use crypto::aes::AesEncryptor;
use std::{
    convert::TryInto,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
};
const CROC_MAGIC: &[u8; 4] = b"croc";
pub struct CrocProto {
    connection: TcpStream,
}
impl CrocProto {
    #![allow(dead_code)]
    pub fn from_stream(connection: TcpStream) -> Self {
        CrocProto {
            connection: connection,
        }
    }
    pub fn connect<A: ToSocketAddrs>(addr: A) -> Result<Self> {
        Ok(CrocProto {
            connection: TcpStream::connect(addr)?,
        })
    }
    pub fn write(&mut self, msg: &[u8]) -> Result<()> {
        let mut buffer = vec![];
        buffer.write_all(CROC_MAGIC)?;
        buffer.write_u32::<LittleEndian>(msg.len().try_into()?)?;
        buffer.write_all(&msg)?;
        self.connection
            .write_all(&buffer)
            .context("Could not send message")?;
        Ok(())
    }
    pub fn read(&mut self) -> Result<Vec<u8>> {
        let mut header_magic = [0u8; 4];
        self.connection
            .read_exact(&mut header_magic)
            .context("Could not read magic")?;
        if &header_magic != CROC_MAGIC {
            return Err(anyhow!("Bad magic {:?}", header_magic));
        }
        let msg_len = self
            .connection
            .read_u32::<LittleEndian>()
            .context("Could not read message size")?;
        let mut message = vec![0u8; msg_len.try_into().unwrap()];
        self.connection.read_exact(message.as_mut_slice())?;
        Ok(message)
    }
}

pub struct EncryptedSession<'a> {
    session: &'a mut CrocProto,
    encryptor: AesEncryptor,
}

impl<'a> EncryptedSession<'a> {
    pub fn new(session: &'a mut CrocProto, session_key: &[u8; 32]) -> Result<EncryptedSession<'a>> {
        let encryptor = AesEncryptor::new(session_key);
        // Let the server know the salt
        session.write(&encryptor.salt)?;
        Ok(Self {
            session: session,
            encryptor: encryptor,
        })
    }
    pub fn write(&mut self, msg: &[u8]) -> Result<()> {
        let encrypted_data = self.encryptor.encrypt(msg)?;
        self.session.write(&encrypted_data)
    }
    pub fn read(&mut self) -> Result<Vec<u8>> {
        self.encryptor.decrypt(&self.session.read()?)
    }
}
