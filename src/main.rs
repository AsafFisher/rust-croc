#![feature(async_closure)]
#![feature(let_chains)]
#![feature(int_roundings)]
//mod cli;
extern crate pretty_env_logger;
#[macro_use]
extern crate log;

mod common;
mod crypto;
mod proto;
mod relay;
use anyhow::Result;
use relay::{client, server};
use std::{env, path::PathBuf, vec};

use crate::proto::{FileInfo, FilesInformation};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info")
    }
    pretty_env_logger::init();
    let relay_task = tokio::task::spawn(async {
        let relay = server::Relay::new(
            "0.0.0.0:9009".to_string(),
            "pass123".to_string(),
            vec![9010],
        );
        relay.start().await.unwrap();
    });

    async fn sender() -> Result<()> {
        let default_relay_addr = "localhost:9009";
        let transferer =
            client::RelayClient::connect(default_relay_addr, "pass123", "12345", false)
                .await?;
        let client = transferer.wait_for_receiver().await?;
        debug!("Start sending");
        let a = client
            .process_client(Some(FilesInformation {
                files_to_transfare: vec![FileInfo {
                    name: "a.txt".to_string(),
                    remote_folder: "./".to_string(),
                    source_folder: "../".to_string(),
                    hash: vec![1, 2, 3],
                    size: PathBuf::from("../a.txt").metadata().unwrap().len() as i64,
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
            client::RelayClient::connect(default_relay_addr, "pass123", "12345", false)
                .await?;
        let client2 = transferer2.connect_to_sender().await?;
        debug!("Start receiving");
        client2.process_client(None).await?;
        Ok(())
    }
    // run relay_task, sender and receiver concurrently and kill relay_task when sender and receiver are done:
    // rust code:

    let (_res2, _res3) = tokio::join!(sender(), receiver());
    relay_task.abort();
    Ok(())
}
