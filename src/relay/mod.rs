pub mod client;
pub mod fs;
pub mod server;

#[cfg(test)]
mod tests {

    use serial_test::serial;
    use std::{io::Write, path::PathBuf};
    use tempfile::NamedTempFile;
    use tokio;

    use crate::{
        proto::{AsyncCrocRead, AsyncCrocWrite, FileInfo, FilesInformation},
        relay::{client, server},
    };
    use anyhow::Result;
    #[tokio::test]
    #[serial]
    async fn test_relay() {
        let relay_task = tokio::task::spawn(async {
            let relay = server::Relay::new(
                "0.0.0.0:9009".to_string(),
                "pass123".to_string(),
                vec![9010],
            );
            relay.start().await.unwrap();
        });

        async fn client_a() -> Result<()> {
            const MSG: &str = "hello";
            let default_relay_addr = "localhost:9009";
            let transferer =
                client::RelayClient::connect(default_relay_addr, "pass123", "12345", false, true)
                    .await?;
            let mut client = transferer.wait_for_receiver().await?;
            debug!("Start sending");
            client.stream.write(MSG.as_bytes()).await?;
            assert_eq!(client.stream.read().await?, MSG.as_bytes());
            Ok(())
        }
        async fn client_b() -> Result<()> {
            let default_relay_addr = "localhost:9009";
            let transferer2: client::RelayClient =
                client::RelayClient::connect(default_relay_addr, "pass123", "12345", false, false)
                    .await?;
            let mut client2 = transferer2.connect_to_sender().await?;
            let buff = client2.stream.read().await?;
            client2.stream.write(buff.as_slice()).await?;
            Ok(())
        }
        let (_res2, _res3) = tokio::join!(client_a(), client_b());
        relay_task.abort();
    }

    #[tokio::test]
    #[serial]
    async fn test_clients() {
        let relay_task = tokio::task::spawn(async {
            let relay = server::Relay::new(
                "0.0.0.0:9009".to_string(),
                "pass123".to_string(),
                vec![9010],
            );
            relay.start().await.unwrap();
        });

        async fn sender(original: PathBuf, dest_file: PathBuf) -> Result<()> {
            let default_relay_addr = "localhost:9009";
            let transferer =
                client::RelayClient::connect(default_relay_addr, "pass123", "12345", false, true)
                    .await?;
            let client = transferer.wait_for_receiver().await?;
            debug!("Start sending");
            let a = client
                .process_client(Some(FilesInformation {
                    files_to_transfare: vec![FileInfo {
                        name: original.file_name().unwrap().to_str().unwrap().to_string(),
                        remote_folder: dest_file.to_str().unwrap().to_string(),
                        source_folder: original.parent().unwrap().to_str().unwrap().to_string(),
                        hash: vec![1, 2, 3],
                        size: original.metadata().unwrap().len() as i64,
                        modification_time: "2021-01-01".to_string(),
                        is_compressed: false,
                        is_encrypted: false,
                        symlink: "".to_string(),
                        mode: 3,
                        temp_file: false,
                    }]
                    .into(),
                    empty_folders_to_transfare: vec![].into(),
                    total_folders_number: 0,
                    machine_id: "123".to_string(),
                    ask: false,
                    sending_text: false,
                    no_compress: true,
                    hash_algorithm: "sha256".to_string(),
                }))
                .await;
            debug!("Returned");
            a?;
            Ok(())
        }
        async fn receiver() -> Result<()> {
            let default_relay_addr = "localhost:9009";
            let transferer2: client::RelayClient =
                client::RelayClient::connect(default_relay_addr, "pass123", "12345", false, false)
                    .await?;
            let client2 = transferer2.connect_to_sender().await?;
            debug!("Start receiving");
            client2.process_client(None).await?;
            Ok(())
        }
        // run relay_task, sender and receiver concurrently and kill relay_task when sender and receiver are done:
        // rust code:
        let directory = tempfile::tempdir().unwrap();
        let mut original = NamedTempFile::new().unwrap();
        original.write(b"hello").unwrap();
        let (_res2, _res3) = tokio::join!(
            sender(original.path().to_owned(), directory.path().to_owned()),
            receiver()
        );
        let mut path_to_dst_file = directory.path().to_owned();
        path_to_dst_file.push(original.path().file_name().unwrap().to_str().unwrap());
        let str = std::fs::read_to_string(path_to_dst_file).unwrap();
        assert_eq!(str, "hello");
        relay_task.abort();
    }
}
