#![forbid(unsafe_code)]

use anyhow::Result;
use token_holdem_sidecar::runtime_supervisor::{run, SupervisorConfig};

#[tokio::main]
async fn main() -> Result<()> {
    run(SupervisorConfig::from_process()?).await
}
