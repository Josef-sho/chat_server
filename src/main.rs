use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::fs::File;
use std::io::Write;
use chrono::Local;
use tokio::time::{Duration, Instant};

// RateLimiter struct for controlling message sending rate per client
struct RateLimiter {
    last_check: Instant,
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64,
}

impl RateLimiter {
    fn new(max_tokens: f64, refill_rate: f64) -> Self {
        RateLimiter {
            last_check: Instant::now(),
            tokens: max_tokens,
            max_tokens,
            refill_rate,
        }
    }

    fn check(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_check).as_secs_f64();
        self.last_check = now;

        self.tokens += elapsed * self.refill_rate;
        if self.tokens > self.max_tokens {
            self.tokens = self.max_tokens;
        }

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

// ClientState struct to hold the client's stream and rate limiter
struct ClientState {
    stream: tokio::net::TcpStream,
    rate_limiter: RateLimiter,
}

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
            let _ = socket.write_all(b"Maximum number of clients reached. Connection rejected.\r\n").await;
            continue;
        }
        *client_count_lock += 1;
        drop(client_count_lock);

        let client_id = addr.to_string();

        tokio::spawn(async move {
            let mut buffer = [0; 1024];

            {
                let mut clients = clients.lock().unwrap();
                clients.insert(
                    client_id.clone(), 
                    ClientState {
                        stream: socket.try_clone().unwrap(), 
                        rate_limiter: RateLimiter::new(5.0, 1.0)  // Limit to 5 messages per second
                    }
                );
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
                        
                        let can_send = {
                            let mut clients = clients.lock().unwrap();
                            if let Some(client_state) = clients.get_mut(&client_id) {
                                client_state.rate_limiter.check()
                            } else {
                                false
                            }
                        };

                        if can_send {
                            if let Some((recipient, message)) = parse_message(&message) {
                                send_private_message(&clients, &recipient, &message, &client_id).await;
                                // Log the private message
                                log_activity(&log_file, &format!("Client {} sent private message to {}: {}", client_id, recipient, message)).await;
                            } else {
                                println!("Invalid message format from {}", client_id);
                            }
                        } else {
                            let _ = socket.write_all(b"Rate limit exceeded. Please wait before sending another message.\r\n").await;
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
async fn broadcast_message(clients: &Arc<Mutex<HashMap<String, ClientState>>>, message: &str) {
    let clients = clients.lock().unwrap();
    for (client_id, client_state) in clients.iter() {
        if let Err(e) = client_state.stream.try_clone().unwrap().write_all(message.as_bytes()).await {
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
    clients: &Arc<Mutex<HashMap<String, ClientState>>>,
    recipient: &str,
    message: &str,
    sender_id: &str,
) {
    let clients = clients.lock().unwrap();
    if let Some(client_state) = clients.get(recipient) {
        let message = format!("{} (private): {}", sender_id, message);
        if let Err(e) = client_state.stream.try_clone().unwrap().write_all(message.as_bytes()).await {
            println!("Failed to send private message to {}: {}", recipient, e);
        }
    } else {
        println!("Recipient {} not found", recipient);
    }
}

// Function to log activity asynchronously
async fn log_activity(log_file: &File, message: &str) {
    let log_message = format!("{} - {}\n", Local::now().format("%Y-%m-%d %H:%M:%S"), message);
    let log_file = log_file.try_clone().unwrap();
    tokio::task::spawn_blocking(move || {
        let _ = log_file.write_all(log_message.as_bytes());
    }).await.unwrap();
}
