use anyhow::{Context, Result};

use inquire::Confirm;
use rust_pake::pake::PakePubKey;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::convert::TryFrom;
use tokio::fs;

use super::{AsyncCrocRead, AsyncCrocWrite, CrocProto};

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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RemoteFileRequest {
    #[serde(rename = "CurrentFileChunkRanges")]
    pub current_file_chunk_ranges: Vec<i64>,
    #[serde(rename = "FilesToTransferCurrentNum")]
    pub files_to_transfer_current_num: i64,
    #[serde(rename = "MachineID")]
    pub machine_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileInfo {
    #[serde(rename = "n")]
    pub name: String,
    #[serde(rename = "fr")]
    pub remote_folder: String,
    #[serde(rename = "fs")]
    pub source_folder: String,
    #[serde(rename = "h")]
    pub hash: Vec<u8>,
    #[serde(rename = "s")]
    pub size: i64,
    #[serde(rename = "m")]
    // Modification time in RFC3339
    pub modification_time: String,
    #[serde(rename = "c")]
    pub is_compressed: bool,
    #[serde(rename = "e")]
    pub is_encrypted: bool,
    #[serde(rename = "sy")]
    pub symlink: String,
    #[serde(rename = "md")]
    pub mode: u32,
    #[serde(rename = "tf")]
    pub temp_file: bool,
}
#[derive(thiserror::Error, Debug)]
enum FileOperationError {
    #[error("Something went wrong with received response {0}")]
    TraversalError(String),
    #[error("User denide file overwrite")]
    OverwriteDenide,
}
impl FileInfo {
    pub async fn create_folder(&self) -> Result<()> {
        let path = fs::canonicalize(&self.remote_folder).await?;
        if !path.starts_with(std::env::current_dir()?) {
            warn!("Path {path:?} is outside of current directory.");
            return Err(
                FileOperationError::TraversalError(path.to_str().unwrap().to_string()).into(),
            );
        }
        if path.exists() {
            if !Confirm::new(&format!("{path:?} exists, do you want to overwrite?")).prompt()? {
                return Err(FileOperationError::OverwriteDenide.into());
            }
        }
        fs::create_dir(path).await?;
        Ok(())
    }
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FilesInformation {
    #[serde(rename = "FilesToTransfer")]
    pub files_to_transfare: Option<Vec<FileInfo>>,
    #[serde(rename = "EmptyFoldersToTransfer")]
    pub empty_folders_to_transfare: Option<Vec<FileInfo>>,
    #[serde(rename = "TotalNumberFolders")]
    pub total_folders_number: usize,
    #[serde(rename = "MachineID")]
    pub machine_id: String,
    #[serde(rename = "Ask")]
    pub ask: bool,
    #[serde(rename = "SendingText")]
    pub sending_text: bool,
    #[serde(rename = "NoCompress")]
    pub no_compress: bool,
    #[serde(rename = "HashAlgorithm")]
    pub hash_algorithm: String,
}

impl FilesInformation {
    pub fn total_items(&self) -> usize {
        return self.files_to_transfare.as_ref().map_or(0, |vec| vec.len())
            + self
                .empty_folders_to_transfare
                .as_ref()
                .map_or(0, |vec| vec.len());
    }
    pub fn total_size(&self) -> i64 {
        // Can have integer overflow here.
        return self
            .files_to_transfare
            .as_ref()
            .map_or(0, |vec| vec.iter().map(|val| val.size).sum());
    }
    pub async fn create_empty_folders(&self) -> Result<()> {
        for file in self.empty_folders_to_transfare.as_ref().unwrap_or(&vec![]) {
            file.create_folder().await?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]

pub struct TypeErrorMessage {
    #[serde(rename = "m")]
    pub(crate) message: String,
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
    #[serde(rename = "fileinfo")]
    FilesInfo(FilesInformation),
    #[serde(rename = "recipientready")]
    TypeRecipientReady(RemoteFileRequest),
    #[serde(rename = "error")]
    TypeError(TypeErrorMessage),
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
}
