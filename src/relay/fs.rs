use std::sync::Arc;

use anyhow::Result;
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    sync::Mutex,
};

use crate::proto::{AsyncCrocRead, AsyncCrocWrite, EncryptedSession, OwnedReceiver, OwnedSender};
pub struct FileChunkInfo {
    pub file: Arc<Mutex<File>>,
    pub chunk_size: usize,
    pub chunk_offset: usize,
}
async fn fs_reader_task(
    mut fs_receiver: tokio::sync::mpsc::Receiver<FileChunkInfo>,
    sender_tx: OwnedSender,
    encrypted_session: EncryptedSession,
) -> Result<()> {
    debug!("fs_reader_task started");
    let encrypted_session_ref = &encrypted_session.clone();
    // Receive a file structure to transmit and sends the actual data to the sender task:
    while let Some(file_chunk_info) = fs_receiver.recv().await {
        let file = file_chunk_info.file;
        let mut chunk = vec![0u8; file_chunk_info.chunk_size];
        {
            let mut file = file.lock().await;
            file.seek(std::io::SeekFrom::Start(
                file_chunk_info.chunk_offset as u64,
            ))
            .await?;
            file.read_exact(&mut chunk).await?;
        }
        // create an encrypt task that will encrypt the chunk and send it to the sender task:
        // spawn task:

        // Construct a buffer that has the position and then the data:
        let mut buffer = file_chunk_info.chunk_offset.to_le_bytes().to_vec();
        buffer.append(&mut chunk);
        let mut sender = sender_tx.clone();
        let encr = encrypted_session_ref.clone();

        // TODO move to encryptor task using IPC
        // encr.
        tokio::task::spawn(async move { encr.write(&mut sender, &buffer).await });
    }
    debug!("fs_reader_task ended");
    Ok(())
}
pub struct FileChunk {
    pub file: Arc<Mutex<File>>,
    pub data: Vec<u8>,
}
async fn fs_writer_task(
    mut fs_receiver: tokio::sync::mpsc::Receiver<FileChunk>,

    encrypted_session: EncryptedSession,
) -> Result<()> {
    debug!("fs_writer_task started");

    // Receive data from the receiver task and write it to the file system directly:
    while let Some(file_chunk) = fs_receiver.recv().await {
        let encryptor = encrypted_session.as_encryptor().clone();
        tokio::spawn(async move {
            let data = encryptor.decrypt(&file_chunk.data);
            match data {
                Ok(data) => {
                    // the opposite of fs_reader_task
                    let file = file_chunk.file;
                    // get offset value from data
                    let mut offset_bytes = [0u8; 8];

                    offset_bytes.copy_from_slice(&data[0..8]);
                    let offset = usize::from_le_bytes(offset_bytes);
                    {
                        let mut file = file.lock().await;
                        file.seek(std::io::SeekFrom::Start(offset as u64))
                            .await
                            .unwrap();
                        file.write_all(&data[8..]).await.unwrap();
                    }
                }
                Err(err) => error!("Could not decrypt chunk {err}"),
            }
        });
    }
    debug!("fs_writer_task ended");
    Ok(())
}

pub struct CrocFsInterface {
    fs_read_message_sender: tokio::sync::mpsc::Sender<FileChunkInfo>,
    fs_write_message_sender: tokio::sync::mpsc::Sender<FileChunk>,
}
impl CrocFsInterface {
    pub async fn new(
        sender_tx: OwnedSender,
        encrypted_session: EncryptedSession,
    ) -> Result<CrocFsInterface> {
        // initialize fs_receiver
        let (fs_read_message_sender, fs_read_message_receiver) = tokio::sync::mpsc::channel(100);
        let (fs_write_message_sender, fs_write_message_receiver) = tokio::sync::mpsc::channel(100);
        // start fs reader task:
        let cloned_encrypted_session = encrypted_session.clone();
        tokio::spawn(async move {
            fs_reader_task(fs_read_message_receiver, sender_tx, encrypted_session).await
        });
        tokio::spawn(async move {
            fs_writer_task(fs_write_message_receiver, cloned_encrypted_session).await
        });
        Ok(CrocFsInterface {
            fs_read_message_sender,
            fs_write_message_sender,
        })
    }
    pub fn into_split(
        self,
    ) -> (
        tokio::sync::mpsc::Sender<FileChunkInfo>,
        tokio::sync::mpsc::Sender<FileChunk>,
    ) {
        (self.fs_read_message_sender, self.fs_write_message_sender)
    }
}

// async fn fs_writer_task(mut receiver_rx: tokio::sync::mpsc::Receiver<Vec<u8>>) -> Result<()> {
//     // Receive data from the receiver task and write it to the file system directly:
//     while let Some(msg) = receiver_rx.recv().await {
//         debug!("Writing to file");
//         fs_handler_transmitter.write(&msg).await?;
//     }
//     Ok(())
// }
