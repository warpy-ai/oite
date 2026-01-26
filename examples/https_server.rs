//! HTTPS Server Example for Benchmarking
//!
//! Usage:
//!   1. Generate test certificates:
//!      ./scripts/gen_test_certs.sh
//!
//!   2. Run the server:
//!      cargo run --release --features tls --example https_server
//!
//!   3. Benchmark with wrk:
//!      wrk -t4 -c100 -d30s https://localhost:8443/
//!
//! Performance Targets:
//!   - 200k+ requests/sec (match Actix-web)
//!   - <1ms average latency
//!   - Session resumption enabled

#[cfg(not(feature = "tls"))]
fn main() {
    eprintln!("Error: TLS feature not enabled");
    eprintln!("Run with: cargo run --release --features tls --example https_server");
    std::process::exit(1);
}

#[cfg(feature = "tls")]
fn main() -> std::io::Result<()> {
    use rustls::ServerConfig;
    use std::io::{BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    println!("==============================================");
    println!("  HTTPS Benchmark Server");
    println!("  Target: 200k+ req/s (Actix-level performance)");
    println!("==============================================");
    println!();

    // Load TLS configuration
    println!("Loading TLS configuration...");
    let cert_path = Path::new("test_certs/cert.pem");
    let key_path = Path::new("test_certs/key.pem");

    if !cert_path.exists() || !key_path.exists() {
        eprintln!("Error: Test certificates not found");
        eprintln!("Generate with: ./scripts/gen_test_certs.sh");
        std::process::exit(1);
    }

    let certs = {
        let cert_file = std::fs::File::open(cert_path)?;
        let mut reader = BufReader::new(cert_file);
        rustls_pemfile::certs(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
    };

    let key = {
        let key_file = std::fs::File::open(key_path)?;
        let mut reader = BufReader::new(key_file);
        rustls_pemfile::private_key(&mut reader)?
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "no private key"))?
    };

    // Build server config with performance optimizations
    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // Performance optimizations
    config.send_tls13_tickets = 2; // Session resumption
    config.session_storage = rustls::server::ServerSessionMemoryCache::new(256);
    config.alpn_protocols = vec![b"http/1.1".to_vec()];

    println!("TLS configuration loaded:");
    println!("  - Certificate: {}", cert_path.display());
    println!("  - Private key: {}", key_path.display());
    println!("  - Session tickets: enabled (2 tickets)");
    println!("  - Session cache: 256 sessions");
    println!("  - ALPN: http/1.1");
    println!();

    let config = Arc::new(config);

    // Bind listener
    let listener = TcpListener::bind("0.0.0.0:8443")?;
    println!("Server listening on https://0.0.0.0:8443/");
    println!();
    println!("Benchmark with:");
    println!("  wrk -t4 -c100 -d30s https://localhost:8443/");
    println!();
    println!("Test with curl:");
    println!("  curl -k https://localhost:8443/");
    println!();

    // Request counter for stats
    let request_count = Arc::new(AtomicU64::new(0));
    let count_clone = request_count.clone();

    // Stats thread
    std::thread::spawn(move || {
        let mut last_count = 0u64;
        let start = std::time::Instant::now();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let current = count_clone.load(Ordering::Relaxed);
            let rps = (current - last_count) / 5;
            let elapsed = start.elapsed().as_secs().max(1);
            let avg_rps = current / elapsed;
            println!(
                "[Stats] Total: {} requests | Last 5s: {} req/s | Avg: {} req/s",
                current, rps, avg_rps
            );
            last_count = current;
        }
    });

    // Accept loop
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let config = config.clone();
                let count = request_count.clone();
                std::thread::spawn(move || {
                    if let Err(e) = handle_connection(stream, &config, &count) {
                        // Connection errors are expected (client disconnects, etc.)
                        let _ = e;
                    }
                });
            }
            Err(e) => {
                eprintln!("Accept error: {}", e);
            }
        }
    }

    Ok(())
}

#[cfg(feature = "tls")]
fn handle_connection(
    mut stream: std::net::TcpStream,
    config: &std::sync::Arc<rustls::ServerConfig>,
    count: &std::sync::atomic::AtomicU64,
) -> std::io::Result<()> {
    use std::io::{Read, Write};
    use std::sync::atomic::Ordering;

    // Perform TLS handshake
    let mut conn = rustls::ServerConnection::new(config.clone())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    // HTTP response
    const RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\n\
Content-Type: text/plain\r\n\
Content-Length: 13\r\n\
Connection: keep-alive\r\n\
\r\n\
Hello, World!";

    let mut buf = [0u8; 4096];
    let mut closed = false;

    while !closed {
        // Read from socket into TLS
        while conn.wants_read() {
            match stream.read(&mut buf) {
                Ok(0) => {
                    closed = true;
                    break;
                }
                Ok(n) => {
                    let mut cursor = std::io::Cursor::new(&buf[..n]);
                    if let Err(_) = conn.read_tls(&mut cursor) {
                        closed = true;
                        break;
                    }
                    if let Err(_) = conn.process_new_packets() {
                        closed = true;
                        break;
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    closed = true;
                    break;
                }
            }
        }

        // Check if handshake is complete
        if !conn.is_handshaking() {
            // Read plaintext request
            let mut request_buf = [0u8; 4096];
            match conn.reader().read(&mut request_buf) {
                Ok(0) => {
                    closed = true;
                }
                Ok(_n) => {
                    // Got a request, send response
                    conn.writer().write_all(RESPONSE)?;
                    count.fetch_add(1, Ordering::Relaxed);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {
                    closed = true;
                }
            }
        }

        // Write from TLS to socket
        while conn.wants_write() {
            match conn.write_tls(&mut stream) {
                Ok(0) => {
                    closed = true;
                    break;
                }
                Ok(_) => {}
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    closed = true;
                    break;
                }
            }
        }

        // If nothing more to do and no data pending, we're done
        if !conn.wants_read() && !conn.wants_write() && !conn.is_handshaking() {
            break;
        }
    }

    Ok(())
}
