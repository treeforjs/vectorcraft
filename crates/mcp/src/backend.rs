//! Where MCP tool calls end up: a control-channel method call.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Something that answers control-channel methods (`engine.execute`, `document.inspect`,
/// `ui.pointer`, `ui.render`, `app.export`, …). See `vectorcraft_ui_egui::control` for the list.
pub trait Backend {
    /// Call one method. `Ok` carries the `result`, `Err` the error message.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String>;
    /// True when a real UI is attached (`ui.*` methods like `ui.key` / `ui.inspect` work).
    fn has_ui(&self) -> bool;
    /// Short human description ("headless", "remote 127.0.0.1:7979").
    fn describe(&self) -> String;
}

/// A running VectorCraft app, reached through its loopback control port.
pub struct Remote {
    addr: String,
    conn: Option<(BufReader<TcpStream>, TcpStream)>,
    next_id: u64,
    last_activity: Instant,
}

impl Remote {
    /// Connect to `addr` (e.g. `127.0.0.1:7979`), failing fast when nothing is listening.
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        let mut r = Self { addr: addr.to_string(), conn: None, next_id: 1, last_activity: Instant::now() };
        r.reconnect()?;
        Ok(r)
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }

    fn reconnect(&mut self) -> std::io::Result<()> {
        self.conn = None;
        let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, format!("cannot resolve {}", self.addr));
        for sa in self.addr.to_socket_addrs()? {
            match TcpStream::connect_timeout(&sa, Duration::from_millis(800)) {
                Ok(s) => {
                    s.set_nodelay(true).ok();
                    // The app answers within 60 s (its own timeout); leave headroom.
                    s.set_read_timeout(Some(Duration::from_secs(90))).ok();
                    let read = s.try_clone()?;
                    self.conn = Some((BufReader::new(read), s));
                    self.last_activity = Instant::now();
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn roundtrip(&mut self, line: &str) -> std::io::Result<String> {
        let Some((reader, writer)) = self.conn.as_mut() else {
            return Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "not connected to the app"));
        };
        writer.write_all(line.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        let mut reply = String::new();
        if reader.read_line(&mut reply)? == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "control channel closed"));
        }
        Ok(reply)
    }
}

impl Backend for Remote {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        // The app expires idle sockets after30s. Renew before sending a new request, with
        // headroom for reply writes, so an ordinary pause never requires replaying an edit.
        if self.conn.is_none() || self.last_activity.elapsed() >= Duration::from_secs(20) {
            self.reconnect().map_err(|e| format!("VectorCraft app at {} is not reachable: {e}", self.addr))?;
        }
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"id": id, "method": method, "params": params}).to_string();
        // Once sending starts, the app may have applied an edit even if its reply is lost.
        // Never replay it automatically; the next explicit call gets a fresh connection.
        let reply = self.roundtrip(&line).map_err(|e| {
            self.conn = None;
            format!("VectorCraft command outcome is unknown: {e}. It was not retried; inspect the document before retrying an edit.")
        })?;
        self.last_activity = Instant::now();
        let v: Value = serde_json::from_str(reply.trim()).map_err(|e| {
            self.conn = None;
            format!(
                "Bad reply from VectorCraft: {e}. The command outcome is unknown; it was not retried. Inspect the document before retrying an edit."
            )
        })?;
        if v.get("id") != Some(&json!(id)) {
            self.conn = None;
            return Err("Mismatched reply from VectorCraft. The command outcome is unknown; it was not retried. Inspect the document before retrying an edit.".into());
        }
        if v.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(v.get("result").cloned().unwrap_or(Value::Null))
        } else {
            Err(v.get("error").and_then(Value::as_str).unwrap_or("unknown error").to_string())
        }
    }

    fn has_ui(&self) -> bool {
        true
    }

    fn describe(&self) -> String {
        format!("remote {}", self.addr)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use std::net::TcpListener;

    fn uncertain_reply_does_not_replay(first_reply: Option<&'static str>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for attempt in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                if attempt == 0 {
                    // The edit was applied, but the client never receives a usable reply.
                    if let Some(reply) = first_reply {
                        writeln!(reader.get_mut(), "{reply}").unwrap();
                    }
                } else {
                    writeln!(reader.get_mut(), "{}", json!({"id":request["id"], "ok":true, "result":{"objects":1}})).unwrap();
                }
                requests.push(request);
            }
            requests
        });

        let mut remote = Remote::connect(&addr).unwrap();
        let error = remote.call("engine.execute", json!({"command":"shape.rectangle", "params":{"width":100,"height":50}})).unwrap_err();
        assert!(error.contains("outcome is unknown") && error.contains("not retried"), "{error}");
        assert!(remote.conn.is_none());
        assert_eq!(remote.call("document.inspect", json!({})).unwrap(), json!({"objects":1}));
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["method"], "engine.execute");
        assert_eq!(requests[1]["method"], "document.inspect", "the edit must not be replayed");
        assert_ne!(requests[0]["id"], requests[1]["id"]);
    }

    #[test]
    fn lost_reply_does_not_replay_an_edit_and_next_call_reconnects() {
        uncertain_reply_does_not_replay(None);
    }

    #[test]
    fn malformed_reply_drops_connection_without_replaying_an_edit() {
        uncertain_reply_does_not_replay(Some("not a JSON reply"));
    }

    #[test]
    fn mismatched_reply_never_certifies_or_replays_an_edit() {
        uncertain_reply_does_not_replay(Some(r#"{"id":999,"ok":true,"result":{}}"#));
    }

    #[test]
    fn idle_connection_is_replaced_before_sending_the_next_edit() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            let (mut idle, _) = listener.accept().unwrap();
            idle.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut idle, &mut bytes).unwrap();
            assert!(bytes.is_empty(), "a stale socket must be closed before the edit is sent");
            let (stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], "engine.execute");
            writeln!(reader.get_mut(), "{}", json!({"id":request["id"],"ok":true,"result":{"objects":1}})).unwrap();
            request
        });
        let mut remote = Remote::connect(&addr).unwrap();
        // Inject elapsed idle time instead of sleeping20s in the regression suite.
        remote.last_activity = Instant::now() - Duration::from_secs(21);
        assert_eq!(remote.call("engine.execute", json!({"command":"shape.rectangle"})).unwrap(), json!({"objects":1}));
        assert_eq!(server.join().unwrap()["id"], 1, "the edit is sent once on the replacement socket");
    }
}
