use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[tokio::main]
async fn main() -> Result<()> {
    let sock = std::path::PathBuf::from("/tmp/kanojo.sock");
    let stream = UnixStream::connect(&sock).await?;
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    writer.write_all(b"Hello from CLI\n").await?;

    if let Some(line) = lines.next_line().await? {
        println!("Received: {}", line);
    }
    Ok(())
}
