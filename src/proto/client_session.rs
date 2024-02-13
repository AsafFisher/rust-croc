use std::{convert::TryInto, sync::Arc};

use crate::crypto::aes::AesEncryptor;
use anyhow::{anyhow, Result};
use inquire::Confirm;
use rand::RngCore;
use rust_pake::pake::{Pake, Role};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{fs::File, sync::Mutex};

use crate::{
    common::config::Config,
    proto::{AsyncCrocRead, AsyncCrocWrite},
    relay::{
        client::RelayClient,
        fs::{CrocFsInterface, FileChunk, FileChunkInfo},
    },
};

use super::{
    croc_msg::{
        ExternalIPMessage, FilesInformation, Message, PakeMessage, RemoteFileRequest,
        TypeErrorMessage,
    },
    croc_raw::{MpscCrocProto, ProtoError},
    CrocProto, EncryptedSession, OwnedSender,
};
const TCP_BUFFER_SIZE: i32 = 1024 * 64;
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
    FileTransfared,
}
trait Process<T> {
    async fn process(self, msg: Message) -> Result<T>;
}

pub struct ClientSession {
    state: ClientState,
    pub stream: CrocProto,
    relay_ports: Vec<String>,
    encrypted_session: Option<EncryptedSession>,
    shared_secret: String,
    pub is_sender: bool,
    external_ip: String,
    peer_external_ip: Option<String>,
    key: Option<Pake<rust_pake::pake::SIEC255Params>>,

    // The whole design here is broken... This struct should be generic
    // in its impl for Receiver and Sender. That way we can maintain one files field that can
    files_to_receive: Option<FilesInformation>,
    config: Config,
}

// receiver_task will receive a message from the client relay and write it to the sender_ipc channel
async fn start_net_task(relay_port: String, shared_secret: String) -> Result<MpscCrocProto> {
    // Connect to relay using relay client
    let default_relay_addr = "localhost";

    // join host and port
    let relay_address = format!("{}:{}", default_relay_addr, relay_port);
    let mut hasher = Sha256::new();
    hasher.update(&shared_secret.as_bytes()[5..]);
    // generate hex string from hash
    let shared_secret = &format!("{:x}", hasher.finalize())[..6];
    debug!("Connecting to relay at {}", relay_address);
    RelayClient::connect(
        &relay_address,
        &"pass123".to_string(),
        &format!("{}-1", shared_secret),
        false,
    )
    .await?
    .start_mpsc_stream()
}
async fn start_fs_task(
    sender_tx: OwnedSender,
    encrypted_session: EncryptedSession,
) -> Result<CrocFsInterface> {
    CrocFsInterface::new(sender_tx, encrypted_session).await
}

impl ClientSession {
    pub fn new(
        stream: CrocProto,
        relay_ports: Vec<String>,
        shared_secret: String,
        // this is redundent and bad
        is_sender: bool,
        external_ip: String,
        config: Option<Config>,
    ) -> Self {
        Self {
            state: ClientState::KeyExchange,
            stream,
            relay_ports,
            encrypted_session: None,
            shared_secret,
            is_sender,
            external_ip,
            peer_external_ip: None,
            key: None,
            files_to_receive: None,
            config: config.unwrap_or_default(),
        }
    }

    // TODO: this should be split to send and recv
    pub async fn process_client(mut self, files: Option<FilesInformation>) -> Result<()> {
        debug!("Starting Client Processing");
        let _port = self
            .relay_ports
            .first()
            .ok_or(anyhow!("Error, no relay port given"))?
            .clone();
        let secret = self.shared_secret.clone();
        let net = start_net_task(self.relay_ports[0].clone(), secret).await?;
        let (mut receiver, sender) = net.into_split();
        let mut rw = None;

        if !self.is_sender {
            debug!("Receiver Started: Sending initial key");
            self.key = Some(Pake::new(
                Role::Sender,
                self.shared_secret[5..].as_bytes().into(),
            ));
            PakeMessage::new(&self.key.as_ref().unwrap().pub_pake, "siec".to_string())?
                .send(&mut self.stream)
                .await?;
        } else {
            debug!("Sender Started: Should get key req");
        }
        loop {
            debug!("Waiting for message");
            self.step()?;
            let msg = Message::recv(&mut self.stream).await?;
            debug!("Got Message");
            self.step()?;
            match msg {
                Message::Pake(msg) => {
                    self.process_key_exchange(msg).await?;
                    let tmp_fs = start_fs_task(
                        sender.clone(),
                        self.encrypted_session.as_ref().unwrap().clone(),
                    )
                    .await?;
                    rw = Some(tmp_fs.into_split());
                }
                Message::ExternalIP(msg) => self.process_ip_exchange(msg).await?,
                Message::Finished => {
                    // send finished
                    Message::Finished.send(&mut self.stream).await?;
                    return Ok(());
                }
                Message::FilesInfo(files) => self.process_files_info(files).await?,
                // Assume files is not none
                Message::TypeRecipientReady(msg) => match &rw {
                    Some((reader, _)) => {
                        self.send_file(&reader, msg, files.as_ref().unwrap())
                            .await?
                    }
                    None => todo!(),
                },
                Message::TypeError(_) => todo!(),
            }
            if self.is_sender && self.state == ClientState::FileInfoTransfare {
                debug!("Sending files info");
                // Again the whole concept of the optional here is just bad.
                Message::FilesInfo(files.clone().unwrap())
                    .send(&mut self.stream)
                    .await?;
                self.state = ClientState::FileTransfare;
            }
            if !self.is_sender && self.state == ClientState::FileTransfare {
                info!("Starting to receive files");
                // loop all files and request them one by one
                for (index, file_info) in self
                    .files_to_receive
                    .as_ref()
                    .unwrap()
                    .files_to_transfare
                    .as_ref()
                    .unwrap()
                    .iter()
                    .enumerate()
                {
                    let remote_path = std::path::Path::new(&file_info.remote_folder)
                        .join(&file_info.name)
                        .to_str()
                        .unwrap()
                        .to_string();
                    debug!("Requesting file: {}", remote_path);
                    // create the file
                    let file = tokio::fs::File::create(remote_path).await?;

                    file.set_len(file_info.size as u64).await?;
                    // request the file
                    Message::TypeRecipientReady(RemoteFileRequest {
                        files_to_transfer_current_num: index as i64,
                        machine_id: "".to_string(),
                        current_file_chunk_ranges: vec![],
                    })
                    .send(&mut self.stream)
                    .await?;

                    let total_chunks = file_info.size.div_ceil(TCP_BUFFER_SIZE as i64);
                    let mut current_amount = 0;
                    let file = Arc::new(Mutex::new(file));
                    // receive the file
                    while current_amount < total_chunks
                        && let Ok(chunk) = receiver.read().await
                    {
                        let mv_file = file.clone();
                        debug!("Current: {current_amount}/{total_chunks}");
                        match &rw {
                            Some((_, writer)) => {
                                writer
                                    .send(FileChunk {
                                        file: mv_file,
                                        data: chunk,
                                    })
                                    .await?;
                                current_amount += 1;
                            }
                            None => panic!("Should not happen"),
                        }
                    }
                    debug!("Done receiving file");
                }
                // send finished
                Message::Finished.send(&mut self.stream).await?;
            }
        }
    }
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
    async fn process_ip_exchange(&mut self, msg: ExternalIPMessage) -> Result<()> {
        if self.state != ClientState::IpExchange {
            // TODO: Make good error
            return Err(anyhow!("Invalid State"));
        }
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
    async fn process_files_info(&mut self, files_info: FilesInformation) -> Result<()> {
        assert!(!self.is_sender);
        if self.state != ClientState::FileInfoTransfare {
            // TODO: Make good error
            return Err(anyhow!("Invalid State"));
        }
        self.files_to_receive = Some(files_info);
        if let Some(files_info) = &self.files_to_receive {
            // TODO: Change files to random names if `Sending Text`
            let files_info_local = files_info.clone();
            let confirmed = tokio::task::spawn_blocking(move || {
                Confirm::new(&format!(
                    "Should receive {} items ({} bytes)",
                    files_info_local.total_items(),
                    files_info_local.total_size()
                ))
                .prompt()
                .unwrap()
            })
            .await?;
            if !confirmed {
                // Notify sender that we did not allow the transaction
                Message::TypeError(TypeErrorMessage {
                    message: "refusing files".to_string(),
                })
                .send(&mut self.stream)
                .await?;
                return Err(anyhow!("Transfare Denied"));
            }
            files_info.create_empty_folders().await?;
            //fs_handler_transmitter.create_empty_folders().await?;
            if files_info.files_to_transfare.is_none() {
                Message::Finished.send(&mut self.stream).await?;
                self.state = ClientState::FileTransfared;
            } else {
                self.state = ClientState::FileTransfare;
            }
        }
        Ok(())
    }
    async fn send_file(
        &mut self,
        reader: &tokio::sync::mpsc::Sender<FileChunkInfo>,
        msg: RemoteFileRequest,
        files: &FilesInformation,
    ) -> Result<()> {
        assert!(self.is_sender);
        assert!(self.state == ClientState::FileTransfare);
        if let Some(files) = &files.files_to_transfare {
            let current_file = &files[msg.files_to_transfer_current_num as usize];
            // join name and folder source using std:
            let file_path =
                std::path::Path::new(&current_file.source_folder).join(&current_file.name);
            debug!("dispatch to send file: {:?}", file_path);
            let file = File::open(file_path).await?;
            // get file size
            let file_size = file.metadata().await?.len();
            let file = Arc::new(Mutex::new(file));
            // send chunks of the file to reader while chunk should be equals or less than TCP_BUFFER_SIZE
            debug!("Sending chunks");
            for chunk_offset in (0..file_size).step_by(TCP_BUFFER_SIZE as usize) {
                let chunk_size = if chunk_offset + TCP_BUFFER_SIZE as u64 > file_size {
                    file_size - chunk_offset
                } else {
                    TCP_BUFFER_SIZE as u64
                };
                reader
                    .send(FileChunkInfo {
                        file: file.clone(),
                        chunk_size: chunk_size as usize,
                        chunk_offset: chunk_offset as usize,
                    })
                    .await?;
                debug!(
                    "Chunk number {}/{} sent",
                    chunk_offset / TCP_BUFFER_SIZE as u64,
                    file_size / TCP_BUFFER_SIZE as u64
                );
            }
            debug!("Finished sending file");
        }
        Ok(())
    }
}
