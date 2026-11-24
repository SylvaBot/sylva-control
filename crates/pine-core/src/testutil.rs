//! Test helpers shared by the client tests.

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Tiny HTTP/1.1 server: each request line is passed to `route`, which returns (status, json body).
pub(crate) async fn mock<F>(route: F) -> String
where
    F: Fn(&str, &str) -> (u16, String) + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let route = Arc::new(route);
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let route = route.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16384];
                let mut n = 0;
                // Read headers, then the body by Content-Length.
                loop {
                    let r = sock.read(&mut buf[n..]).await.unwrap();
                    if r == 0 { return; }
                    n += r;
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let len = text.lines()
                            .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                            .unwrap_or(0);
                        if n >= end + 4 + len { break; }
                    }
                }
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                let (head, body) = text.split_once("\r\n\r\n").unwrap();
                let (status, out) = route(head, body);
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
                    out.len()
                );
                sock.write_all(resp.as_bytes()).await.unwrap();
            });
        }
    });
    format!("http://{addr}")
}

