use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use std::error::Error;
use tokio::io::stdin;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut stream = TcpStream::connect("127.0.0.1:8080").await?;
    println!("Connected to server");

    // Create a channel for sending messages to the server
    let (tx, mut rx) = mpsc::channel::<String>(100);

    // Clone the stream for the writing task
    let (write_stream, mut write_stream_clone) = stream.split();

    // Spawn a task to read user input and send to server
    tokio::spawn(async move {
        let mut stdin = stdin();
        let mut buffer = vec![0; 1024];
        loop {
            match stdin.read(&mut buffer).await {
                Ok(n) if n > 0 => {
                    let input = String::from_utf8_lossy(&buffer[..n]);
                    if let Err(e) = write_stream_clone.write_all(input.as_bytes()).await {
                        eprintln!("Failed to send message: {}", e);
                        break;
                    }
                }
                _ => {}
            }
        }
    });

    // Main task to read from server
    let mut buffer = [0; 1024];
    loop {
        match stream.read(&mut buffer).await {
            Ok(0) => {
                println!("Server closed the connection");
                break;
            }
            Ok(n) => {
                let message = String::from_utf8_lossy(&buffer[..n]);
                println!("{}", message);
            }
            Err(e) => {
                eprintln!("Failed to read from server: {}", e);
                break;
            }
        }
    }

    Ok(())
}
