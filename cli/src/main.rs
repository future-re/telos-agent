#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telos_cli::run().await
}
