//! Real TCP regression tests for the client side of the desktop control protocol.
//! All peers are deterministic loopback fixtures; no GUI, sound device or external host.

use serde_json::{Value, json};
use soundcraft_automation::{Backend, Remote};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

fn request(reader: &mut BufReader<TcpStream>) -> Value {
    let mut line = String::new();
    assert!(reader.read_line(&mut line).unwrap() > 0);
    serde_json::from_str(&line).unwrap()
}

fn reply(mut stream: &TcpStream, value: Value) {
    writeln!(stream, "{value}").unwrap();
}

#[test]
fn remote_correlates_two_sequential_requests_on_one_connection() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut read = BufReader::new(stream.try_clone().unwrap());
        let first = request(&mut read);
        assert_eq!(first, json!({"id": 1, "method": "session.inspect", "params": {"detail": "full"}}));
        reply(&stream, json!({"id": 1, "ok": true, "result": {"tracks": 2}}));
        let second = request(&mut read);
        assert_eq!(second, json!({"id": 2, "method": "engine.execute", "params": {"command": "track.new"}}));
        reply(&stream, json!({"id": 2, "ok": true, "result": {"created": true}}));
    });

    let mut remote = Remote::new(&address.to_string());
    assert_eq!(remote.call("session.inspect", json!({"detail": "full"})).unwrap(), json!({"tracks": 2}));
    assert_eq!(remote.call("engine.execute", json!({"command": "track.new"})).unwrap(), json!({"created": true}));
    server.join().unwrap();
}

#[test]
fn remote_rejects_wrong_response_id_and_reconnects_on_next_explicit_request() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (first, _) = listener.accept().unwrap();
        let mut read = BufReader::new(first.try_clone().unwrap());
        assert_eq!(request(&mut read)["id"], 1);
        reply(&first, json!({"id": 999, "ok": true, "result": {"wrong": true}}));
        drop(read);
        drop(first);

        let (second, _) = listener.accept().unwrap();
        let mut read = BufReader::new(second.try_clone().unwrap());
        assert_eq!(request(&mut read)["id"], 2);
        reply(&second, json!({"id": 2, "ok": true, "result": {"correct": true}}));
    });

    let mut remote = Remote::new(&address.to_string());
    let error = remote.call("session.inspect", json!({})).unwrap_err();
    assert!(error.contains("id mismatch"), "{error}");
    assert_eq!(remote.call("session.inspect", json!({})).unwrap(), json!({"correct": true}));
    server.join().unwrap();
}

#[test]
fn backend_error_containing_closed_does_not_replay_mutating_command() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (first, _) = listener.accept().unwrap();
        let mut read = BufReader::new(first.try_clone().unwrap());
        let sent = request(&mut read);
        assert_eq!(sent["method"], "engine.execute");
        reply(&first, json!({"id": 1, "ok": false, "error": "clip closed for editing"}));

        // An implicit retry on a new socket is an unsafe second execution.
        listener.set_nonblocking(true).unwrap();
        let end = Instant::now() + Duration::from_millis(400);
        let mut replays = 0;
        while Instant::now() < end {
            match listener.accept() {
                Ok((stream, _)) => {
                    let mut again = BufReader::new(stream.try_clone().unwrap());
                    let second = request(&mut again);
                    replays += 1;
                    reply(&stream, json!({"id": second["id"], "ok": true, "result": "unexpected replay"}));
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(5)),
                Err(e) => panic!("accept failed: {e}"),
            }
        }
        replays
    });

    let mut remote = Remote::new(&address.to_string());
    let error = remote.call("engine.execute", json!({"command": "clip.delete"})).unwrap_err();
    assert_eq!(error, "clip closed for editing");
    assert_eq!(server.join().unwrap(), 0, "the backend command was replayed");
}

#[test]
fn remote_does_not_replay_after_response_eof_but_can_reconnect_next_call() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (first, _) = listener.accept().unwrap();
        let mut read = BufReader::new(first.try_clone().unwrap());
        assert_eq!(request(&mut read)["id"], 1);
        drop(read);
        drop(first);

        let (second, _) = listener.accept().unwrap();
        let mut read = BufReader::new(second.try_clone().unwrap());
        assert_eq!(request(&mut read)["id"], 2);
        reply(&second, json!({"id": 2, "ok": true, "result": "fresh call"}));
    });

    let mut remote = Remote::new(&address.to_string());
    assert!(remote.call("engine.execute", json!({"command": "track.new"})).is_err());
    assert_eq!(remote.call("session.inspect", json!({})).unwrap(), json!("fresh call"));
    server.join().unwrap();
}

#[test]
fn oversized_control_request_is_rejected_before_any_tcp_connection() {
    // Port 1 is intentionally unreachable, so this distinguishes local size validation
    // from a connection error without involving any real server.
    let mut remote = Remote::new("127.0.0.1:1");
    let error = remote.call("session.inspect", json!({"payload": "x".repeat(1024 * 1024)})).unwrap_err();
    assert_eq!(error, "control request exceeds 1 MiB limit");
}

#[test]
fn remote_rejects_truncated_json_even_when_it_is_valid_without_a_newline() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        assert_eq!(request(&mut reader)["id"], 1);
        stream.write_all(b"{\"id\":1,\"ok\":true,\"result\":99}").unwrap();
        // EOF without the required JSON-lines delimiter is a transport failure, not success.
    });

    let mut remote = Remote::new(&address.to_string());
    let error = remote.call("engine.execute", json!({"command": "track.new"})).unwrap_err();
    assert!(error.contains("before complete control response"), "{error}");
    server.join().unwrap();
}

#[test]
fn remote_rejects_oversized_response_before_decoding_it_as_json() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        assert_eq!(request(&mut reader)["id"], 1);
        // Send more than the 8-MiB client limit without a newline.
        // A peer disconnect is expected once the client notices the limit.
        let payload = vec![b' '; 8 * 1024 * 1024 + 8192];
        let _ = stream.write_all(&payload);
    });

    let mut remote = Remote::new(&address.to_string());
    let error = remote.call("session.inspect", json!({})).unwrap_err();
    assert!(error.contains("exceeds 8 MiB"), "{error}");
    server.join().unwrap();
}
