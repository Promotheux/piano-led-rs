//! Web UI + JSON API for the tablet app.
//!
//! The UI files are embedded in the binary. The tablet sends commands with
//! plain HTTP and receives live state (player clock, pressed keys) over a
//! push-only WebSocket, so each connection only ever writes.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tiny_http::{Header, Method, Request, Response, Server, SslConfig, StatusCode};
use tungstenite::handshake::derive_accept_key;
use tungstenite::protocol::Role;
use tungstenite::{Message, WebSocket};

use crate::midi;
use crate::notes::NoteState;
use crate::player::{self, Command, Player, Status};
use crate::settings::{Settings, SettingsStore};

const MAX_UPLOAD: u64 = 1 << 20;
const MAX_JSON: u64 = 64 * 1024;
const PUSH_INTERVAL: Duration = Duration::from_millis(50);

static UI: &[(&str, &str, &[u8])] = &[
    ("/", "text/html; charset=utf-8", include_bytes!("../ui/index.html")),
    ("/app.js", "text/javascript; charset=utf-8", include_bytes!("../ui/app.js")),
    ("/app.css", "text/css; charset=utf-8", include_bytes!("../ui/app.css")),
    ("/sw.js", "text/javascript; charset=utf-8", include_bytes!("../ui/sw.js")),
    ("/manifest.webmanifest", "application/manifest+json", include_bytes!("../ui/manifest.webmanifest")),
    ("/icon.svg", "image/svg+xml", include_bytes!("../ui/icon.svg")),
    ("/icon-192.png", "image/png", include_bytes!("../ui/icon-192.png")),
    ("/icon-512.png", "image/png", include_bytes!("../ui/icon-512.png")),
];

pub struct App {
    pub notes: Arc<NoteState>,
    pub settings: Arc<SettingsStore>,
    pub player: Arc<Player>,
    pub songs_dir: PathBuf,
    /// Local CA certificate (DER) to install on tablets, when HTTPS is on.
    pub ca_der: Option<Vec<u8>>,
}

type Resp = Response<std::io::Cursor<Vec<u8>>>;

pub fn serve(addr: &str, app: Arc<App>) -> Result<(), String> {
    let server = Server::http(addr).map_err(|e| format!("http {addr}: {e}"))?;
    spawn(server, app, "plv-http")
}

/// Same app over TLS, so Android's Chrome treats it as installable.
pub fn serve_https(addr: &str, app: Arc<App>, cert_chain_pem: Vec<u8>, key_pem: Vec<u8>) -> Result<(), String> {
    let config = SslConfig { certificate: cert_chain_pem, private_key: key_pem };
    let server = Server::https(addr, config).map_err(|e| format!("https {addr}: {e}"))?;
    spawn(server, app, "plv-https")
}

fn spawn(server: Server, app: Arc<App>, name: &str) -> Result<(), String> {
    std::thread::Builder::new()
        .name(name.into())
        .stack_size(256 * 1024)
        .spawn(move || {
            for req in server.incoming_requests() {
                handle(&app, req);
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn handle(app: &Arc<App>, mut req: Request) {
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    if path == "/ws" {
        return websocket(app, req);
    }
    let resp = match (req.method(), path) {
        (Method::Get, p) if UI.iter().any(|u| u.0 == p) => {
            let (_, ctype, body) = UI.iter().find(|u| u.0 == p).unwrap();
            Response::from_data(body.to_vec()).with_header(header("Content-Type", ctype))
        }
        (Method::Get, "/ca.crt") => match &app.ca_der {
            Some(der) => Response::from_data(der.clone())
                .with_header(header("Content-Type", "application/x-x509-ca-cert"))
                .with_header(header("Content-Disposition", "attachment; filename=\"piano-led-ca.crt\"")),
            None => error(404, "HTTPS is off"),
        },
        (Method::Get, "/api/songs") => list_songs(&app.songs_dir),
        (Method::Post, "/api/songs") => upload(app, &mut req, query),
        (Method::Delete, "/api/songs") => delete_song(app, query),
        (Method::Get, "/api/settings") => json(&app.settings.get()),
        (Method::Put, "/api/settings") => put_settings(app, &mut req),
        (Method::Get, "/api/ports") => match midi::list_ports() {
            Ok(inputs) => json(&serde_json::json!({ "inputs": inputs })),
            Err(e) => error(500, &e),
        },
        (Method::Get, "/api/player") => json(&app.player.status()),
        (Method::Post, "/api/player") => post_player(app, &mut req),
        (Method::Get, "/api/player/song") => match app.player.song_json() {
            Some(s) => Response::from_string(s).with_header(header("Content-Type", "application/json")),
            None => error(404, "no song loaded"),
        },
        _ => error(404, "not found"),
    };
    let _ = req.respond(resp.with_header(header("Cache-Control", "no-cache")));
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("static header")
}

fn json<T: Serialize>(v: &T) -> Resp {
    Response::from_string(serde_json::to_string(v).unwrap_or_default())
        .with_header(header("Content-Type", "application/json"))
}

fn error(code: u16, msg: &str) -> Resp {
    json(&serde_json::json!({ "error": msg })).with_status_code(StatusCode(code))
}

fn read_body(req: &mut Request, limit: u64) -> Result<Vec<u8>, Resp> {
    let mut body = Vec::new();
    req.as_reader()
        .take(limit + 1)
        .read_to_end(&mut body)
        .map_err(|e| error(400, &e.to_string()))?;
    if body.len() as u64 > limit {
        return Err(error(413, "body too large"));
    }
    Ok(body)
}

fn put_settings(app: &App, req: &mut Request) -> Resp {
    let body = match read_body(req, MAX_JSON) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let new: Settings = match serde_json::from_slice(&body) {
        Ok(s) => s,
        Err(e) => return error(400, &e.to_string()),
    };
    let reach = new.hand_reach;
    match app.settings.set(new) {
        Ok(()) => {
            let _ = app.player.command(Command { reach: Some(reach), ..Default::default() });
            json(&app.settings.get())
        }
        Err(e) => error(400, &e),
    }
}

#[derive(Deserialize)]
struct PlayerRequest {
    /// Load this song (file name in the songs folder) before applying `cmd`.
    song: Option<String>,
    #[serde(flatten)]
    cmd: Command,
}

fn post_player(app: &App, req: &mut Request) -> Resp {
    let body = match read_body(req, MAX_JSON) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let r: PlayerRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return error(400, &e.to_string()),
    };
    if let Some(name) = r.song {
        let Some(path) = song_path(&app.songs_dir, &name) else {
            return error(400, "invalid song name");
        };
        let song = match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| player::parse(&b)) {
            Ok(s) => s,
            Err(e) => return error(422, &e),
        };
        app.player.load(name, song);
    }
    match app.player.command(r.cmd) {
        Ok(()) => json(&app.player.status()),
        Err(e) => error(400, &e),
    }
}

/// Maps a client-supplied file name into the songs folder, refusing anything
/// that could escape it.
fn song_path(dir: &Path, name: &str) -> Option<PathBuf> {
    let lower = name.to_ascii_lowercase();
    let ok = !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && !name.contains(['/', '\\', '\0', ':'])
        && (lower.ends_with(".mid") || lower.ends_with(".midi"));
    ok.then(|| dir.join(name))
}

fn list_songs(dir: &Path) -> Resp {
    #[derive(Serialize)]
    struct Entry {
        name: String,
        size: u64,
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return error(500, &format!("cannot read songs folder {}", dir.display()));
    };
    let mut songs: Vec<Entry> = rd
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            song_path(dir, &name)?;
            Some(Entry { size: e.metadata().ok()?.len(), name })
        })
        .collect();
    songs.sort_by_key(|e| e.name.to_lowercase());
    json(&serde_json::json!({ "songs": songs }))
}

/// The `name=` query parameter, decoded.
fn name_param(query: &str) -> String {
    query.split('&').find_map(|kv| kv.strip_prefix("name=")).map(percent_decode).unwrap_or_default()
}

/// An existing song with exactly these bytes, if any. Only files of the same
/// size are read, so this stays cheap even for hundreds of songs.
fn find_duplicate(dir: &Path, body: &[u8]) -> Option<String> {
    std::fs::read_dir(dir).ok()?.filter_map(Result::ok).find_map(|e| {
        let name = e.file_name().into_string().ok()?;
        song_path(dir, &name)?;
        let same_size = e.metadata().ok()?.len() == body.len() as u64;
        (same_size && std::fs::read(e.path()).ok()? == body).then_some(name)
    })
}

fn delete_song(app: &App, query: &str) -> Resp {
    let name = name_param(query);
    let Some(path) = song_path(&app.songs_dir, &name) else {
        return error(400, "invalid song name");
    };
    match std::fs::remove_file(&path) {
        Ok(()) => json(&serde_json::json!({ "deleted": name })),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => error(404, "no such song"),
        Err(e) => error(500, &format!("delete failed: {e}")),
    }
}

fn upload(app: &App, req: &mut Request, query: &str) -> Resp {
    let name = name_param(query);
    let Some(path) = song_path(&app.songs_dir, &name) else {
        return error(400, "name must be a .mid file name");
    };
    if path.exists() {
        return error(409, "a song with that name already exists");
    }
    let body = match read_body(req, MAX_UPLOAD) {
        Ok(b) => b,
        Err(r) => return r,
    };
    if let Err(e) = player::parse(&body) {
        return error(422, &e);
    }
    if let Some(existing) = find_duplicate(&app.songs_dir, &body) {
        return error(409, &format!("already in your library as {existing}"));
    }
    // shortcut: no free-space check; a full disk surfaces as a write error.
    let tmp = path.with_extension("upload.tmp");
    if let Err(e) = std::fs::write(&tmp, &body).and_then(|_| std::fs::rename(&tmp, &path)) {
        let _ = std::fs::remove_file(&tmp);
        return error(507, &format!("save failed: {e}"));
    }
    json(&serde_json::json!({ "name": name }))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b[i], hex) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (b'+', _) => {
                out.push(b' ');
                i += 1;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[derive(Serialize)]
struct Live {
    #[serde(flatten)]
    status: Status,
    held: Vec<u8>,
    /// Bumps on every settings change, so open tablets reload them.
    settings_gen: u32,
}

fn websocket(app: &Arc<App>, req: Request) {
    let key = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Sec-WebSocket-Key"))
        .map(|h| h.value.as_str().to_string());
    let Some(key) = key else {
        let _ = req.respond(error(400, "expected a WebSocket upgrade"));
        return;
    };
    let resp = Response::empty(StatusCode(101))
        .with_header(header("Sec-WebSocket-Accept", &derive_accept_key(key.as_bytes())));
    let stream = req.upgrade("websocket", resp);
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("plv-ws".into())
        .stack_size(128 * 1024)
        .spawn(move || {
            let mut ws = WebSocket::from_raw_socket(stream, Role::Server, None);
            loop {
                let live = Live {
                    status: app.player.status(),
                    held: (0u8..128).filter(|&k| app.notes.held(k as usize) != 0).collect(),
                    settings_gen: app.settings.generation(),
                };
                let msg = serde_json::to_string(&live).unwrap_or_default();
                // The client went away (or the network did): end this thread.
                if ws.send(Message::text(msg)).is_err() {
                    break;
                }
                std::thread::sleep(PUSH_INTERVAL);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn song_paths_stay_inside_the_folder() {
        let d = Path::new("songs");
        assert_eq!(song_path(d, "Für Elise.mid"), Some(d.join("Für Elise.mid")));
        assert!(song_path(d, "x.MIDI").is_some());
        for bad in ["../x.mid", "a/b.mid", "a\\b.mid", ".hidden.mid", "x.txt", "", "c:x.mid"] {
            assert_eq!(song_path(d, bad), None, "{bad}");
        }
    }

    #[test]
    fn duplicates_are_found_by_content() {
        let dir = std::env::temp_dir().join(format!("plv-dup-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.mid"), b"MThd-one").unwrap();
        std::fs::write(dir.join("b.mid"), b"MThd-two").unwrap(); // same size, other bytes
        std::fs::write(dir.join("notes.txt"), b"MThd-one").unwrap(); // not a song
        assert_eq!(find_duplicate(&dir, b"MThd-one").as_deref(), Some("a.mid"));
        assert_eq!(find_duplicate(&dir, b"MThd-new"), None);
        assert_eq!(find_duplicate(&dir, b"different length"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("F%C3%BCr+Elise.mid"), "Für Elise.mid");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%2"), "%2");
    }
}
