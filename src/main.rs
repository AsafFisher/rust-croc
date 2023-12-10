//mod cli;
extern crate pretty_env_logger;
#[macro_use]
extern crate log;

mod proto;
mod relay;
use anyhow::Result;
use relay::{client, server};
use std::vec;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();
    let relay_task = tokio::task::spawn(async {
        let relay = server::Relay::new("0.0.0.0:9009", "pass123".to_string(), vec![9010, 9110]);
        relay.start().await.unwrap();
    });

    async fn sender() -> Result<()> {
        let default_relay_addr = "localhost:9009";
        let transferer =
            client::RelayClient::connect(default_relay_addr, "pass123", "12345", false, true)
                .await?;
        let mut client = transferer.wait_for_receiver().await?;
        debug!("Start sanding");
        let a = client.process_client().await;
        debug!("Returned");
        a?;
        Ok(())
    }
    async fn receiver() -> Result<()> {
        let default_relay_addr = "localhost:9009";
        let transferer2 =
            client::RelayClient::connect(default_relay_addr, "pass123", "12345", false, false)
                .await?;
        let mut client2 = transferer2.connect_to_sender().await?;
        debug!("Start receiving");
        client2.process_client().await?;
        Ok(())
    }
    let (res1, res2, res3) = tokio::join!(relay_task, sender(), receiver());
    Ok(())
}
