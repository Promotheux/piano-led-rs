//! Piano LED Visualizer engine.
//!
//! Threads (all spawned once at start-up):
//!   * midir backend thread  – decodes MIDI, writes `NoteState` atomics.
//!   * render thread         – envelopes + LED buffer + SPI, small fixed stack.
//!   * player thread         – song clock, MIDI out, LED hints.
//!   * http thread (+1 per open tablet) – web UI and live state.
//!   * main thread           – supervisor: (re)connects the MIDI ports.

mod color;
mod config;
mod hal;
mod keymap;
mod midi;
mod notes;
mod player;
mod render;
mod settings;
mod tls;
mod web;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keymap::KeyMap;
use midi::{MidiListener, MidiOut};
use notes::NoteState;
use player::Player;
use render::Renderer;
use settings::{Settings, SettingsStore};

/// Render thread stack. All its buffers are on the heap or inline in
/// `Renderer` (~2 KB), so the 2 MB Rust default is pure waste.
const RENDER_STACK: usize = 64 * 1024;
const RECONNECT_POLL: Duration = Duration::from_secs(2);

fn main() {
    let cfg = match config::parse(std::env::args().skip(1)) {
        Ok(c) => c,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("error: {e}\n");
            }
            eprint!("{}", config::USAGE);
            std::process::exit(if e.is_empty() { 0 } else { 2 });
        }
    };

    if cfg.list_ports {
        match midi::list_ports() {
            Ok(ports) if ports.is_empty() => println!("(no MIDI inputs)"),
            Ok(ports) => ports.iter().for_each(|p| println!("{p}")),
            Err(e) => fatal(&e),
        }
        return;
    }

    let running = Arc::new(AtomicBool::new(true));
    {
        let r = running.clone();
        ctrlc::set_handler(move || r.store(false, Ordering::Relaxed))
            .unwrap_or_else(|e| fatal(&format!("signal handler: {e}")));
    }

    // Saved UI settings win over flags; flags only seed the first run.
    let store = Arc::new(SettingsStore::load(cfg.settings_path.clone(), Settings::from_config(&cfg)));
    let initial = store.get();
    let led_count = initial.led_count;

    let notes = Arc::new(NoteState::new());
    let driver = hal::open(cfg.driver, led_count, cfg.hw).unwrap_or_else(|e| fatal(&e));
    let mut renderer = Renderer::new(
        cfg.render.clone(),
        KeyMap::new(&initial.geometry(led_count)),
        initial.color_mode().note_table(),
    );
    renderer.apply(&initial);

    let render_thread = {
        let (n, s, r) = (notes.clone(), store.clone(), running.clone());
        std::thread::Builder::new()
            .name("plv-render".into())
            .stack_size(RENDER_STACK)
            .spawn(move || render::run(renderer, driver, n, s, r))
            .unwrap_or_else(|e| fatal(&format!("spawn render thread: {e}")))
    };
    let render_handle = render_thread.thread().clone();

    let player = Arc::new(Player::new());
    let _ = player.command(player::Command { reach: Some(initial.hand_reach), ..Default::default() });
    let midi_out: Arc<Mutex<Option<MidiOut>>> = Arc::new(Mutex::new(None));
    let player_thread = {
        let (p, n, o, r) = (player.clone(), notes.clone(), midi_out.clone(), running.clone());
        std::thread::Builder::new()
            .name("plv-player".into())
            .stack_size(128 * 1024)
            .spawn(move || player::run(p, n, o, r))
            .unwrap_or_else(|e| fatal(&format!("spawn player thread: {e}")))
    };

    // Certificates live next to the settings file (/var/lib/plv on the Pi).
    let tls = cfg.https.as_ref().and_then(|_| {
        let dir = cfg.settings_path.as_ref().and_then(|p| p.parent()).filter(|d| !d.as_os_str().is_empty());
        tls::load_or_create(dir.unwrap_or(std::path::Path::new("."))).map_err(|e| eprintln!("web: {e}")).ok()
    });
    let app = Arc::new(web::App {
        notes: notes.clone(),
        settings: store.clone(),
        player: player.clone(),
        songs_dir: cfg.songs_dir.clone(),
        ca_der: tls.as_ref().map(|t| t.ca_der.clone()),
    });
    if let Some(addr) = &cfg.http {
        match web::serve(addr, app.clone()) {
            Ok(()) => eprintln!("web: http://{addr}/ (songs in {})", cfg.songs_dir.display()),
            Err(e) => eprintln!("web: {e}"),
        }
    }
    if let (Some(addr), Some(t)) = (&cfg.https, tls) {
        match web::serve_https(addr, app, t.cert_chain_pem, t.key_pem) {
            Ok(()) => eprintln!("web: https://{addr}/ for {}", t.names.join(", ")),
            Err(e) => eprintln!("web: {e}"),
        }
    }

    eprintln!("plv: {led_count} LEDs, {:?} driver, {} fps", cfg.driver, cfg.render.fps);

    if cfg.demo {
        demo(&notes, &running, &render_handle);
    } else {
        supervise_midi(&store, &notes, &midi_out, &running, render_handle);
    }

    running.store(false, Ordering::Relaxed);
    render_thread.thread().unpark();
    let _ = player_thread.join();
    let _ = render_thread.join();
    eprintln!();
}

/// Keeps the MIDI connections alive across keyboard power cycles / unplugs,
/// and reconnects when the port is changed in the UI.
fn supervise_midi(
    store: &SettingsStore,
    notes: &Arc<NoteState>,
    midi_out: &Mutex<Option<MidiOut>>,
    running: &AtomicBool,
    render: std::thread::Thread,
) {
    let mut conn: Option<MidiListener> = None;
    let mut port = store.get().midi_port;
    let mut seen_gen = store.generation();
    let mut warned = false;
    while running.load(Ordering::Relaxed) {
        if store.generation() != seen_gen {
            seen_gen = store.generation();
            let wanted = store.get().midi_port;
            if wanted != port {
                port = wanted;
                conn = None;
                *midi_out.lock().unwrap() = None;
                notes.all_off();
            }
        }
        match &conn {
            Some(c) if !c.still_present() => {
                eprintln!("midi: '{}' disconnected", c.port_name);
                conn = None;
                *midi_out.lock().unwrap() = None;
                notes.all_off();
            }
            Some(_) => {}
            None => match MidiListener::connect(port.as_deref(), notes.clone(), Some(render.clone())) {
                Ok(Some(c)) => {
                    eprintln!("midi: listening on '{}'", c.port_name);
                    conn = Some(c);
                    warned = false;
                }
                Ok(None) if !warned => {
                    eprintln!("midi: no input port yet, waiting...");
                    warned = true;
                }
                Ok(None) => {}
                Err(e) => eprintln!("midi: {e}"),
            },
        }
        // Output follows the input: the player sends to the same instrument.
        if conn.is_some() && midi_out.lock().unwrap().is_none() {
            match MidiOut::connect(port.as_deref()) {
                Ok(Some(o)) => {
                    eprintln!("midi: sending to '{}'", o.port_name);
                    *midi_out.lock().unwrap() = Some(o);
                }
                Ok(None) => {}
                Err(e) => eprintln!("midi: {e}"),
            }
        }
        sleep_while(running, RECONNECT_POLL);
    }
}

/// Plays an up/down scale through the same code path as real MIDI input.
fn demo(notes: &NoteState, running: &AtomicBool, render: &std::thread::Thread) {
    let mut note = 21u8;
    let mut up = true;
    while running.load(Ordering::Relaxed) {
        midi::handle_message(&[0x90, note, 100], notes);
        render.unpark();
        sleep_while(running, Duration::from_millis(60));
        midi::handle_message(&[0x80, note, 0], notes);
        match (up, note) {
            (true, 108) => up = false,
            (false, 21) => up = true,
            _ => {}
        }
        note = if up { note + 1 } else { note - 1 };
    }
}

fn sleep_while(running: &AtomicBool, total: Duration) {
    let step = Duration::from_millis(50);
    let mut left = total;
    while running.load(Ordering::Relaxed) && !left.is_zero() {
        let s = left.min(step);
        std::thread::sleep(s);
        left -= s;
    }
}

fn fatal(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}
