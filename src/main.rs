use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    rusty_apple_mail_mcp::run().await
}
