//! Loopback JSON-lines control server: one request per line, one reply per line.
//! This is the transport the MCP server (`soundcraft-cli mcp --connect PORT`) wraps.

use serde_json::{Value, json};
use soundcraft_ui_egui::ControlRequest;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

pub fn start(port: u16, ctx: egui::Context) -> Receiver<ControlRequest> {
    let (tx, rx) = channel::<ControlRequest>();
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            log::error!("control server failed to bind 127.0.0.1:{port}: {e}");
            return rx;
        }
    };
    let actual = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    log::info!("control server listening on 127.0.0.1:{actual}");
    println!("SOUNDCRAFT_CONTROL_PORT={actual}");
    let active = Arc::new(AtomicUsize::new(0));
    let spawned = std::thread::Builder::new().name("soundcraft-control".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let Some(permit) = ConnectionPermit::acquire(&active, MAX_CONTROL_CONNECTIONS) else {
                // A peer must not be able to create unbounded blocked connection threads.
                drop(stream);
                continue;
            };
            let tx = tx.clone();
            let ctx = ctx.clone();
            let spawned = std::thread::Builder::new().name("soundcraft-control-conn".into()).spawn(move || {
                let _permit = permit;
                serve(stream, tx, ctx);
            });
            if let Err(err) = spawned {
                log::warn!("control connection thread failed: {err}");
            }
        }
    });
    if let Err(e) = spawned {
        log::error!("control thread failed: {e}");
    }
    rx
}

/// Bound the per-client threads as well as the per-message byte allocation.
const MAX_CONTROL_CONNECTIONS: usize = 32;

/// An RAII permit releases a slot on EOF, send error, panic, or spawn failure.
struct ConnectionPermit(Arc<AtomicUsize>);

impl ConnectionPermit {
    fn acquire(active: &Arc<AtomicUsize>, limit: usize) -> Option<Self> {
        active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| (count < limit).then_some(count + 1))
            .ok()
            .map(|_| Self(Arc::clone(active)))
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Includes the newline: large or unterminated JSON lines must not grow memory indefinitely.
const MAX_CONTROL_REQUEST_BYTES: usize = 1024 * 1024;

/// Read one bounded JSON-lines frame. Oversized frames are fully discarded up to their newline
/// so the next client request can still be processed on the same connection.
fn read_limited_line(reader: &mut impl BufRead) -> std::io::Result<Option<Result<String, &'static str>>> {
    let mut data = Vec::new();
    let mut oversized = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if data.is_empty() && !oversized {
                return Ok(None);
            }
            break;
        }
        let consumed = available.iter().position(|&b| b == b'\n').map_or(available.len(), |n| n + 1);
        let complete = available[consumed - 1] == b'\n';
        if !oversized {
            if data.len().saturating_add(consumed) > MAX_CONTROL_REQUEST_BYTES {
                oversized = true;
                data.clear();
            } else {
                data.extend_from_slice(&available[..consumed]);
            }
        }
        reader.consume(consumed);
        if complete {
            break;
        }
    }
    if oversized {
        return Ok(Some(Err("control request exceeds 1 MiB limit")));
    }
    Ok(Some(String::from_utf8(data).map_err(|_| "control request is not UTF-8")))
}

fn serve(stream: TcpStream, tx: Sender<ControlRequest>, ctx: egui::Context) {
    let Ok(read) = stream.try_clone() else { return };
    let mut out = stream;
    let mut reader = BufReader::new(read);
    loop {
        let line = match read_limited_line(&mut reader) {
            Ok(Some(Ok(line))) => line,
            Ok(Some(Err(error))) => {
                if writeln!(out, "{}", json!({"ok": false, "error": error})).is_err() {
                    break;
                }
                continue;
            }
            Ok(None) | Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => {
                let id = msg.get("id").cloned().unwrap_or(Value::Null);
                let method = msg.get("method").and_then(Value::as_str).unwrap_or("").to_string();
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                let (req, rrx) = ControlRequest::new(method, params);
                if tx.send(req).is_err() {
                    break;
                }
                ctx.request_repaint();
                let mut r = rrx.recv_timeout(Duration::from_secs(90)).unwrap_or_else(|_| json!({"ok": false, "error": "timeout"}));
                if let Some(o) = r.as_object_mut() {
                    o.insert("id".into(), id);
                }
                r
            }
            Err(e) => json!({"ok": false, "error": format!("bad JSON: {e}")}),
        };
        if writeln!(out, "{reply}").is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_limit_rejects_overflow_then_reuses_released_slot() {
        let active = Arc::new(AtomicUsize::new(0));
        let held: Vec<_> = (0..MAX_CONTROL_CONNECTIONS)
            .map(|_| ConnectionPermit::acquire(&active, MAX_CONTROL_CONNECTIONS).unwrap())
            .collect();
        assert_eq!(active.load(Ordering::SeqCst), MAX_CONTROL_CONNECTIONS);
        assert!(ConnectionPermit::acquire(&active, MAX_CONTROL_CONNECTIONS).is_none());
        drop(held);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        let acquired = ConnectionPermit::acquire(&active, MAX_CONTROL_CONNECTIONS).unwrap();
        assert_eq!(active.load(Ordering::SeqCst), 1);
        drop(acquired);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn connection_limit_is_atomic_under_concurrent_accepts() {
        let active = Arc::new(AtomicUsize::new(0));
        let start = Arc::new(std::sync::Barrier::new(17));
        let acquired = Arc::new(std::sync::Barrier::new(17));
        let release = Arc::new(std::sync::Barrier::new(17));
        let mut handles = Vec::new();
        for _ in 0..16 {
            let active = Arc::clone(&active);
            let start = Arc::clone(&start);
            let acquired = Arc::clone(&acquired);
            let release = Arc::clone(&release);
            handles.push(std::thread::spawn(move || {
                start.wait();
                let permit = ConnectionPermit::acquire(&active, 4);
                acquired.wait();
                release.wait();
                permit.is_some()
            }));
        }
        start.wait();
        acquired.wait();
        assert_eq!(active.load(Ordering::SeqCst), 4);
        release.wait();
        let admitted = handles.into_iter().filter(|h| h.join().unwrap()).count();
        assert_eq!(admitted, 4, "only four simultaneous connections may be admitted");
        assert_eq!(active.load(Ordering::SeqCst), 0, "all slots must be returned");
    }

    #[test]
    fn bounded_json_lines_reject_oversized_and_invalid_utf8_without_losing_next_line() {
        let mut input = Vec::new();
        input.extend_from_slice(&vec![b'a'; MAX_CONTROL_REQUEST_BYTES - 1]);
        input.push(b'\n');
        input.extend_from_slice(&vec![b'b'; MAX_CONTROL_REQUEST_BYTES]);
        input.extend_from_slice(b"\n");
        input.extend_from_slice(b"\xff\n");
        input.extend_from_slice(b"{\"id\":3}\n");
        let mut reader = BufReader::new(input.as_slice());
        assert_eq!(read_limited_line(&mut reader).unwrap().unwrap().unwrap().len(), MAX_CONTROL_REQUEST_BYTES);
        assert_eq!(read_limited_line(&mut reader).unwrap().unwrap(), Err("control request exceeds 1 MiB limit"));
        assert_eq!(read_limited_line(&mut reader).unwrap().unwrap(), Err("control request is not UTF-8"));
        assert_eq!(read_limited_line(&mut reader).unwrap().unwrap(), Ok("{\"id\":3}\n".to_string()));
        assert!(read_limited_line(&mut reader).unwrap().is_none());
    }

    #[test]
    fn oversized_tcp_request_is_rejected_and_following_request_is_processed() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = channel::<ControlRequest>();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve(stream, sender, egui::Context::default());
        });

        let mut client = TcpStream::connect(address).unwrap();
        client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut read = BufReader::new(client.try_clone().unwrap());
        client.write_all(&vec![b'x'; MAX_CONTROL_REQUEST_BYTES]).unwrap();
        client.write_all(b"\n").unwrap();

        let mut line = String::new();
        read.read_line(&mut line).unwrap();
        let error: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(error, json!({"ok": false, "error": "control request exceeds 1 MiB limit"}));

        writeln!(client, "{}", json!({"id": "after-overflow", "method": "session.inspect"})).unwrap();
        let request = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(request.method, "session.inspect");
        request.reply.send(json!({"ok": true, "result": 42})).unwrap();

        line.clear();
        read.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply, json!({"id": "after-overflow", "ok": true, "result": 42}));

        drop(read);
        drop(client);
        server.join().unwrap();
    }

    /// A real TCP client exercises the JSON-lines transport, error responses, request
    /// correlation and asynchronous handoff without starting a window or audio device.
    #[test]
    fn tcp_control_handles_bad_json_and_round_trips_real_requests() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = channel::<ControlRequest>();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve(stream, sender, egui::Context::default());
        });

        let mut client = TcpStream::connect(address).unwrap();
        client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut read = BufReader::new(client.try_clone().unwrap());

        writeln!(client).unwrap(); // Blank lines never produce spurious responses.
        writeln!(client, "{{invalid-json").unwrap();
        let mut line = String::new();
        read.read_line(&mut line).unwrap();
        let invalid: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(invalid["ok"], false);
        assert!(invalid["error"].as_str().unwrap().contains("bad JSON"));

        writeln!(client, "{}", json!({"id": 77, "method": "session.inspect", "params": {"detail": "full"}})).unwrap();
        let request = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(request.method, "session.inspect");
        assert_eq!(request.params["detail"], "full");
        request.reply.send(json!({"ok": true, "result": {"name": "QA"}})).unwrap();

        line.clear();
        read.read_line(&mut line).unwrap();
        let result: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(result, json!({"id": 77, "ok": true, "result": {"name": "QA"}}));

        drop(read);
        drop(client);
        server.join().unwrap();
    }
}
