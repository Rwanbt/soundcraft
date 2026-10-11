//! Agent control for SoundCraft.
//!
//! - [`Backend`]: something that answers control-protocol methods (`engine.execute`,
//!   `session.inspect`, `ui.*`…). [`Headless`] runs an in-process engine; [`Remote`] talks to a
//!   running app's `--control` port.
//! - [`mcp::Server`]: a Model Context Protocol server over stdio (newline-delimited JSON-RPC 2.0)
//!   exposing SoundCraft as tools and resources.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod mcp;

use serde_json::{Value, json};
use soundcraft_engine::Engine;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Answers control-protocol methods.
pub trait Backend {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String>;
    /// True when a UI is attached (screenshots, clicks).
    fn has_ui(&self) -> bool;
    fn describe(&self) -> String;
}

/// An in-process engine (no UI).
pub struct Headless {
    pub engine: Engine,
}

impl Headless {
    pub fn new(engine: Engine) -> Self {
        Headless { engine }
    }
}

impl Backend for Headless {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        match method {
            "engine.execute" | "command" => {
                let id = params.get("command").or_else(|| params.get("id")).and_then(Value::as_str).ok_or("`command` required")?;
                let p = params.get("params").cloned().unwrap_or(json!({}));
                let r = self.engine.execute(id, &p).map_err(|e| e.to_string());
                // Headless has no audio engine: drop transport requests but keep the playhead.
                self.engine.transport_requests.clear();
                r
            }
            "engine.commands" => self.engine.execute("engine.commands", &params).map_err(|e| e.to_string()),
            "engine.parity" => Ok(soundcraft_engine::catalog::parity_json()),
            "session.inspect" | "document.inspect" => {
                Ok(soundcraft_engine::inspect::session(&self.engine, params.get("detail").and_then(Value::as_str) == Some("full")))
            }
            m if m.starts_with("ui.") => {
                Err(format!("`{m}` needs the desktop app: start `soundcraft --control 0` and use `soundcraft-cli mcp --connect PORT`"))
            }
            other => Err(format!("unknown method `{other}`")),
        }
    }
    fn has_ui(&self) -> bool {
        false
    }
    fn describe(&self) -> String {
        format!("headless engine, session `{}`", self.engine.session().name)
    }
}

/// A TCP client for a running app's control channel.
pub struct Remote {
    addr: String,
    conn: Option<(TcpStream, BufReader<TcpStream>)>,
    next_id: u64,
}

impl Remote {
    /// `target` is a port (`7979`) or `host:port`.
    pub fn new(target: &str) -> Self {
        let addr = if target.contains(':') { target.to_string() } else { format!("127.0.0.1:{target}") };
        Remote { addr, conn: None, next_id: 1 }
    }

    fn connect(&mut self) -> Result<(), String> {
        if self.conn.is_some() {
            return Ok(());
        }
        let sock = self.addr.parse().map_err(|e| format!("{}: {e}", self.addr))?;
        let s =
            TcpStream::connect_timeout(&sock, Duration::from_millis(800)).map_err(|e| format!("cannot reach SoundCraft at {}: {e}", self.addr))?;
        s.set_read_timeout(Some(Duration::from_secs(120))).map_err(|e| e.to_string())?;
        let r = s.try_clone().map_err(|e| e.to_string())?;
        self.conn = Some((s, BufReader::new(r)));
        Ok(())
    }

    fn roundtrip(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        self.connect()?;
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"id": id, "method": method, "params": params}).to_string();
        let (w, r) = self.conn.as_mut().ok_or("not connected")?;
        writeln!(w, "{line}").map_err(|e| e.to_string())?;
        let mut buf = String::new();
        r.read_line(&mut buf).map_err(|e| e.to_string())?;
        if buf.is_empty() {
            return Err("connection closed".into());
        }
        let v: Value = serde_json::from_str(&buf).map_err(|e| e.to_string())?;
        // Reject stale, out-of-order and misrouted replies instead of applying a
        // different request's result. A mismatched stream is unsafe to reuse.
        if v.get("id").and_then(Value::as_u64) != Some(id) {
            self.conn = None;
            return Err(format!("control reply ID mismatch: expected {id}"));
        }
        if v.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            Ok(v.get("result").cloned().unwrap_or(Value::Null))
        } else {
            Err(v.get("error").and_then(Value::as_str).unwrap_or("error").to_string())
        }
    }
}

impl Backend for Remote {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        match self.roundtrip(method, &params) {
            Ok(v) => Ok(v),
            Err(e) if e.contains("closed") || e.contains("Broken pipe") || e.contains("reset") => {
                // Retry once on a fresh connection.
                self.conn = None;
                self.roundtrip(method, &params)
            }
            Err(e) => Err(e),
        }
    }
    fn has_ui(&self) -> bool {
        true
    }
    fn describe(&self) -> String {
        format!("SoundCraft app at {}", self.addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_executes_and_inspects() {
        let mut h = Headless::new(Engine::default());
        let r = h.call("engine.execute", json!({"command": "track.new", "params": {"count": 2, "format": "stereo"}}));
        assert!(r.is_ok(), "{r:?}");
        let s = h.call("session.inspect", json!({})).unwrap();
        assert_eq!(s["tracks"].as_array().map(Vec::len), Some(2));
        assert!(h.call("ui.click", json!({})).is_err());
        assert!(h.call("nope", json!({})).is_err());
    }

    #[test]
    fn remote_reports_unreachable() {
        let mut r = Remote::new("127.0.0.1:1");
        assert!(r.call("session.inspect", json!({})).is_err());
    }
}

#[cfg(test)]
mod remote_protocol_regressions {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn remote_reuses_a_socket_and_correlates_sequential_requests() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            for id in 1..=2 {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["id"], id);
                assert_eq!(request["method"], "session.inspect");
                writeln!(stream, "{}", json!({"id": id, "ok": true, "result": {"sequence": id}})).unwrap();
            }
        });
        let mut remote = Remote::new(&port.to_string());
        assert!(remote.has_ui());
        assert_eq!(remote.call("session.inspect", json!({})).unwrap()["sequence"], 1);
        assert_eq!(remote.call("session.inspect", json!({})).unwrap()["sequence"], 2);
        server.join().unwrap();
    }

    #[test]
    fn remote_rejects_a_wrong_id_and_reconnects_cleanly_on_next_call() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut first, _) = listener.accept().unwrap();
            let mut first_read = BufReader::new(first.try_clone().unwrap());
            let mut line = String::new();
            first_read.read_line(&mut line).unwrap();
            assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["id"], 1);
            writeln!(first, "{}", json!({"id": 999, "ok": true, "result": "wrong request"})).unwrap();
            drop(first_read);
            drop(first);
            let (mut next, _) = listener.accept().unwrap();
            let mut next_read = BufReader::new(next.try_clone().unwrap());
            line.clear();
            next_read.read_line(&mut line).unwrap();
            assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["id"], 2);
            writeln!(next, "{}", json!({"id": 2, "ok": true, "result": "correct"})).unwrap();
        });
        let mut remote = Remote::new(&port.to_string());
        let err = remote.call("session.inspect", json!({})).unwrap_err();
        assert!(err.contains("ID mismatch"), "{err}");
        assert!(remote.conn.is_none(), "mismatched response invalidates the socket");
        assert_eq!(remote.call("session.inspect", json!({})).unwrap(), json!("correct"));
        server.join().unwrap();
    }

    #[test]
    fn remote_retries_once_when_a_connection_closes_without_a_reply() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["id"], 1);
            drop(reader);
            let (mut second, _) = listener.accept().unwrap();
            let mut read = BufReader::new(second.try_clone().unwrap());
            line.clear();
            read.read_line(&mut line).unwrap();
            assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["id"], 2);
            writeln!(second, "{}", json!({"id": 2, "ok": true, "result": 123})).unwrap();
        });
        let mut remote = Remote::new(&port.to_string());
        assert_eq!(remote.call("session.inspect", json!({})).unwrap(), json!(123));
        server.join().unwrap();
    }

    #[test]
    fn remote_surfaces_remote_errors_and_invalid_json_without_panicking() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut read = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            read.read_line(&mut line).unwrap();
            writeln!(stream, "{}", json!({"id": 1, "ok": false, "error": "permission denied"})).unwrap();
            line.clear();
            read.read_line(&mut line).unwrap();
            writeln!(stream, "{{ invalid json").unwrap();
        });
        let mut remote = Remote::new(&port.to_string());
        assert_eq!(remote.call("session.inspect", json!({})).unwrap_err(), "permission denied");
        assert!(remote.call("session.inspect", json!({})).is_err());
        server.join().unwrap();
    }
}
