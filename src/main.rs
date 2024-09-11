use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::fs::File;
use std::io::Write;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    let clients = Arc::new(Mutex::new(HashMap::new()));
    let max_clients = 5;
    let client_count = Arc::new(Mutex::new(0));

    println!("Server listening on 127.0.0.1:8080");

    // Create a file for logging
    let log_file = File::create("chat_server.log")?;

    loop {
        let (mut socket, addr) = listener.accept().await?;
        println!("Client connected from: {}", addr);

        let clients = clients.clone();
        let client_count = client_count.clone();
        let log_file = log_file.try_clone()?;

        let mut client_count_lock = client_count.lock().unwrap();
        if *client_count_lock >= max_clients {
            println!("Maximum number of clients reached. Rejecting connection from {}", addr);
            let _ = socket.write_all(b"Maximum number of clients reached. Connection rejected.\r\n");
            continue;
        }
        *client_count_lock += 1;
        drop(client_count_lock);

        tokio::spawn(async move {
            let mut buffer = [0; 1024];
            let client_id = addr.to_string();  // Use the client's address as an ID

            {
                let mut clients = clients.lock().unwrap();
                clients.insert(client_id.clone(), socket.try_clone().unwrap());  // Store the client's socket
            }

            // Log client connection
            log_activity(&log_file, &format!("Client {} connected", client_id)).await;

            // Notify other clients about the new connection
            broadcast_message(&clients, &format!("Client {} connected", client_id)).await;

            // Handle client communication
            loop {
                match socket.read(&mut buffer).await {
                    Ok(n) if n == 0 => {
                        println!("Client {} disconnected", client_id);
                        // Log client disconnection
                        log_activity(&log_file, &format!("Client {} disconnected", client_id)).await;
                        break;
                    }
                    Ok(n) => {
                        let message = String::from_utf8_lossy(&buffer[..n]);
                        println!("Raw message: {}", message); // Log the raw message for debugging
                        if let Some((recipient, message)) = parse_message(&message) {
                            send_private_message(&clients, &recipient, &message, &client_id).await;
                            // Log the private message
                            log_activity(&log_file, &format!("Client {} sent private message to {}: {}", client_id, recipient, message)).await;
                        } else {
                            println!("Invalid message format from {}", client_id);
                        }
                    }
                    Err(e) => {
                        println!("Error reading from client {}: {}", client_id, e);
                        break;
                    }
                }
            }

            // Remove client from the set
            let mut clients = clients.lock().unwrap();
            clients.remove(&client_id);
            drop(clients);

            let mut client_count_lock = client_count.lock().unwrap();
            *client_count_lock -= 1;
        });
    }
}

// Function to broadcast messages to all clients
async fn broadcast_message(clients: &Arc<Mutex<HashMap<String, tokio::net::TcpStream>>>, message: &str) {
    let clients = clients.lock().unwrap();
    for (client_id, socket) in clients.iter() {
        if let Err(e) = socket.try_clone().unwrap().write_all(message.as_bytes()).await {
            println!("Failed to send message to {}: {}", client_id, e);
        }
    }
}

// Function to parse message for recipient and content
fn parse_message(message: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = message.trim().splitn(2, " ").collect();
    if parts.len() == 2 {
        Some((parts[0].to_string(), parts[1].trim().to_string()))
    } else {
        None
    }
}

// Function to send a private message
async fn send_private_message(
    clients: &Arc<Mutex<HashMap<String, tokio::net::TcpStream>>>,
    recipient: &str,
    message: &str,
    sender_id: &str,
) {
    let clients = clients.lock().unwrap();
    if let Some(socket) = clients.get(recipient) {
        let message = format!("{} (private): {}", sender_id, message);
        if let Err(e) = socket.try_clone().unwrap().write_all(message.as_bytes()).await {
            println!("Failed to send private message to {}: {}", recipient, e);
        }
    } else {
        println!("Recipient {} not found", recipient);
    }
}

// Function to log activity
async fn log_activity(log_file: &File, message: &str) {
    let mut log_file = log_file.try_clone().unwrap();
    let _ = log_file.write_all(format!("{} - {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), message).as_bytes());
}