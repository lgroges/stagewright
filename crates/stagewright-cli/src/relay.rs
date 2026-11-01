//! `stagewright relay`: the co-writing relay. A standard y-websocket endpoint:
//! clients connect to `ws://host:port/<room>` and exchange Yjs sync and
//! awareness messages. The relay keeps each room's document (so a writer can
//! catch up when the other is offline) and saves it to `--data`.
//!
//! The room name is the invite secret: 22+ random characters, never listed.

use futures_util::{SinkExt, StreamExt};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio_tungstenite::tungstenite::Message as Ws;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use yrs::encoding::read::Cursor;
use yrs::sync::{Awareness, Message, MessageReader, SyncMessage};
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::updates::encoder::Encode;
use yrs::{Doc, ReadTxn, StateVector, Transact, Update};

struct Room {
    awareness: Awareness,
    peers: HashMap<u64, UnboundedSender<Vec<u8>>>,
    dirty: bool,
}

type Rooms = Arc<Mutex<HashMap<String, Arc<Mutex<Room>>>>>;

/// Room names are secrets chosen by the app: long, URL-safe, nothing else.
pub fn valid_room(name: &str) -> bool {
    (22..=128).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn room_file(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.ydoc"))
}

fn load_room(dir: &Path, name: &str) -> Room {
    let doc = Doc::new();
    if let Ok(bytes) = std::fs::read(room_file(dir, name)) {
        match Update::decode_v1(&bytes) {
            Ok(u) => {
                if let Err(e) = doc.transact_mut().apply_update(u) {
                    eprintln!("relay: room {name}: could not apply saved state: {e}");
                }
            }
            Err(e) => eprintln!("relay: room {name}: saved state unreadable: {e}"),
        }
    }
    Room {
        awareness: Awareness::new(doc),
        peers: HashMap::new(),
        dirty: false,
    }
}

fn save_room(dir: &Path, name: &str, room: &mut Room) {
    if !room.dirty {
        return;
    }
    let state = room
        .awareness
        .doc()
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
    let path = room_file(dir, name);
    let tmp = path.with_extension("ydoc.tmp");
    let ok = std::fs::write(&tmp, &state).and_then(|_| std::fs::rename(&tmp, &path));
    match ok {
        Ok(()) => room.dirty = false,
        Err(e) => eprintln!("relay: could not save room {name}: {e}"),
    }
}

fn broadcast(room: &Room, from: u64, bytes: &[u8]) {
    for (id, tx) in &room.peers {
        if *id != from {
            let _ = tx.send(bytes.to_vec());
        }
    }
}

/// Handle one binary frame from `conn`. Returns replies for the sender.
fn process(room: &mut Room, conn: u64, data: &[u8], my_clients: &mut HashSet<u64>) -> Vec<Vec<u8>> {
    let mut replies = Vec::new();
    let mut decoder = DecoderV1::new(Cursor::new(data));
    let reader = MessageReader::new(&mut decoder);
    for next in reader {
        let msg = match next {
            Ok(m) => m,
            Err(e) => {
                eprintln!("relay: bad message: {e}");
                break;
            }
        };
        match msg {
            Message::Sync(SyncMessage::SyncStep1(sv)) => {
                let update = room.awareness.doc().transact().encode_state_as_update_v1(&sv);
                replies.push(Message::Sync(SyncMessage::SyncStep2(update)).encode_v1());
            }
            Message::Sync(SyncMessage::SyncStep2(bytes)) | Message::Sync(SyncMessage::Update(bytes)) => {
                let applied = Update::decode_v1(&bytes).map_err(|e| e.to_string()).and_then(|u| {
                    room.awareness
                        .doc()
                        .transact_mut()
                        .apply_update(u)
                        .map_err(|e| e.to_string())
                });
                match applied {
                    Ok(()) => {
                        room.dirty = true;
                        broadcast(room, conn, &Message::Sync(SyncMessage::Update(bytes)).encode_v1());
                    }
                    Err(e) => eprintln!("relay: rejected update: {e}"),
                }
            }
            Message::Awareness(update) => {
                my_clients.extend(update.clients.keys().map(|c| c.get()));
                let encoded = Message::Awareness(update.clone()).encode_v1();
                if room.awareness.apply_update(update).is_ok() {
                    broadcast(room, conn, &encoded);
                }
            }
            Message::AwarenessQuery => {
                if let Ok(u) = room.awareness.update() {
                    replies.push(Message::Awareness(u).encode_v1());
                }
            }
            Message::Auth(_) | Message::Custom(_, _) => {}
        }
    }
    replies
}

// The handshake callback's error type is tungstenite's (large) HTTP response.
#[allow(clippy::result_large_err)]
async fn connection(stream: TcpStream, rooms: Rooms, data: PathBuf, conn: u64) {
    let mut path = String::new();
    let accepted = tokio_tungstenite::accept_hdr_async(stream, |req: &Request, resp: Response| {
        path = req.uri().path().trim_start_matches('/').to_string();
        if valid_room(&path) {
            Ok(resp)
        } else {
            let mut err = ErrorResponse::new(Some("unknown room".into()));
            *err.status_mut() = tokio_tungstenite::tungstenite::http::StatusCode::NOT_FOUND;
            Err(err)
        }
    })
    .await;
    let ws = match accepted {
        Ok(ws) => ws,
        Err(_) => return,
    };
    let room = {
        let mut map = rooms.lock().unwrap();
        map.entry(path.clone())
            .or_insert_with(|| Arc::new(Mutex::new(load_room(&data, &path))))
            .clone()
    };

    let (mut sink, mut source) = ws.split();
    let (tx, mut rx) = unbounded_channel::<Vec<u8>>();
    {
        let mut r = room.lock().unwrap();
        r.peers.insert(conn, tx.clone());
        // Start the handshake: our state vector, then everyone's presence.
        let sv = r.awareness.doc().transact().state_vector();
        let _ = tx.send(Message::Sync(SyncMessage::SyncStep1(sv)).encode_v1());
        if let Ok(u) = r.awareness.update()
            && !u.clients.is_empty()
        {
            let _ = tx.send(Message::Awareness(u).encode_v1());
        }
    }
    let writer = tokio::spawn(async move {
        while let Some(bytes) = rx.recv().await {
            if sink.send(Ws::Binary(bytes.into())).await.is_err() {
                break;
            }
        }
    });

    let mut my_clients = HashSet::new();
    while let Some(Ok(msg)) = source.next().await {
        match msg {
            Ws::Binary(data) => {
                let replies = {
                    let mut r = room.lock().unwrap();
                    process(&mut r, conn, &data, &mut my_clients)
                };
                for reply in replies {
                    let _ = tx.send(reply);
                }
            }
            Ws::Close(_) => break,
            _ => {}
        }
    }

    // Disconnected: drop this writer's cursors for everyone else, save the room.
    {
        let mut r = room.lock().unwrap();
        r.peers.remove(&conn);
        let gone: Vec<yrs::block::ClientID> = my_clients.iter().map(|c| yrs::block::ClientID::new(*c)).collect();
        for c in &gone {
            r.awareness.remove_state(*c);
        }
        if let Ok(u) = r.awareness.update_with_clients(gone.iter().copied()) {
            broadcast(&r, conn, &Message::Awareness(u).encode_v1());
        }
        if r.peers.is_empty() {
            save_room(&data, &path, &mut r);
        }
    }
    drop(tx);
    writer.abort();
}

pub async fn serve(listener: TcpListener, data: PathBuf) {
    let rooms: Rooms = Arc::default();
    // Save changed rooms every few seconds.
    {
        let rooms = rooms.clone();
        let data = data.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(3)).await;
                let list: Vec<(String, Arc<Mutex<Room>>)> = rooms
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                for (name, room) in list {
                    save_room(&data, &name, &mut room.lock().unwrap());
                }
            }
        });
    }
    let next = AtomicU64::new(1);
    while let Ok((stream, _)) = listener.accept().await {
        let id = next.fetch_add(1, Ordering::Relaxed);
        tokio::spawn(connection(stream, rooms.clone(), data.clone(), id));
    }
}

pub fn run(host: String, port: u16, data: PathBuf) -> Result<(), String> {
    std::fs::create_dir_all(&data).map_err(|e| format!("{}: {e}", data.display()))?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    rt.block_on(async move {
        let listener = TcpListener::bind((host.as_str(), port))
            .await
            .map_err(|e| e.to_string())?;
        eprintln!(
            "stagewright relay: ws://{host}:{port}/<room>  (rooms saved in {})",
            data.display()
        );
        serve(listener, data).await;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::connect_async;
    use yrs::{GetString, Text};

    const ROOM: &str = "test-room-0123456789abcdef";

    async fn start(data: PathBuf) -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(serve(listener, data));
        port
    }

    /// A minimal y-websocket client: sync with the relay, apply what it sends.
    struct Client {
        doc: Doc,
        ws: tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
    }

    impl Client {
        async fn connect(port: u16) -> Client {
            let (ws, _) = connect_async(format!("ws://127.0.0.1:{port}/{ROOM}")).await.unwrap();
            let mut c = Client { doc: Doc::new(), ws };
            let sv = c.doc.transact().state_vector();
            c.send(Message::Sync(SyncMessage::SyncStep1(sv)).encode_v1()).await;
            c
        }
        async fn send(&mut self, bytes: Vec<u8>) {
            self.ws.send(Ws::Binary(bytes.into())).await.unwrap();
        }
        /// Read frames for a while, applying updates and answering step 1.
        async fn pump(&mut self) {
            while let Ok(Some(Ok(Ws::Binary(data)))) =
                tokio::time::timeout(Duration::from_millis(300), self.ws.next()).await
            {
                let mut decoder = DecoderV1::new(Cursor::new(&data));
                let mut reader = MessageReader::new(&mut decoder);
                let mut out = Vec::new();
                while let Some(Ok(m)) = reader.next() {
                    match m {
                        Message::Sync(SyncMessage::SyncStep1(sv)) => {
                            let u = self.doc.transact().encode_state_as_update_v1(&sv);
                            out.push(Message::Sync(SyncMessage::SyncStep2(u)).encode_v1());
                        }
                        Message::Sync(SyncMessage::SyncStep2(u)) | Message::Sync(SyncMessage::Update(u)) => {
                            self.doc
                                .transact_mut()
                                .apply_update(Update::decode_v1(&u).unwrap())
                                .unwrap();
                        }
                        _ => {}
                    }
                }
                for o in out {
                    self.send(o).await;
                }
            }
        }
        async fn type_text(&mut self, s: &str) {
            let text = self.doc.get_or_insert_text("t");
            let update = {
                let mut txn = self.doc.transact_mut();
                let len = text.len(&txn);
                text.insert(&mut txn, len, s);
                txn.encode_update_v1()
            };
            self.send(Message::Sync(SyncMessage::Update(update)).encode_v1()).await;
        }
        fn text(&self) -> String {
            let t = self.doc.get_or_insert_text("t");
            t.get_string(&self.doc.transact())
        }
    }

    #[tokio::test]
    async fn two_writers_converge_and_late_joiner_catches_up_after_restart() {
        let dir = tempfile_dir();
        let port = start(dir.clone()).await;
        let mut a = Client::connect(port).await;
        let mut b = Client::connect(port).await;
        a.pump().await;
        b.pump().await;
        a.type_text("Hello").await;
        b.type_text(", world").await;
        a.pump().await;
        b.pump().await;
        assert_eq!(a.text(), b.text());
        assert_eq!(a.text().len(), "Hello, world".len());
        drop(a);
        drop(b);
        tokio::time::sleep(Duration::from_millis(300)).await;
        // A fresh relay on the same data folder still has the room.
        let port2 = start(dir.clone()).await;
        let mut c = Client::connect(port2).await;
        c.pump().await;
        assert_eq!(c.text().len(), "Hello, world".len());
    }

    #[tokio::test]
    async fn unknown_room_names_are_refused() {
        let port = start(tempfile_dir()).await;
        assert!(connect_async(format!("ws://127.0.0.1:{port}/short")).await.is_err());
        assert!(
            connect_async(format!("ws://127.0.0.1:{port}/{}", "x".repeat(30) + "/../etc"))
                .await
                .is_err()
        );
    }

    fn tempfile_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("sw-relay-test-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn rand_suffix() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64
    }

    #[test]
    fn room_names() {
        assert!(valid_room("abcdefghijklmnopqrstuv"));
        assert!(!valid_room("abc"));
        assert!(!valid_room("abcdefghijklmnopqrstuv/.."));
    }
}
