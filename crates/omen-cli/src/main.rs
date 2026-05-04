use anyhow::Result;
use omen_core::{Request, Response, SOCKET_PATH};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

fn main() -> Result<()> {
    let req = Request::GetSnapshot;
    let mut stream = UnixStream::connect(SOCKET_PATH)?;
    let payload = serde_json::to_string(&req)?;
    stream.write_all(payload.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    let resp: Response = serde_json::from_str(&line)?;
    println!("{:#?}", resp);
    Ok(())
}
