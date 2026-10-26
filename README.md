# plv — Piano LED Visualizer in Rust

A small, fast rewrite of
[onlaj/Piano-LED-Visualizer](https://github.com/onlaj/Piano-LED-Visualizer),
built for the **Raspberry Pi Zero 2 W**. It reads MIDI from your piano, lights a
WS2812B strip above the keys, and serves a **tablet app** with a
Synthesia-style practice player. It all runs as one ~1 MB binary that uses about
3 MB of RAM.

The original's LCD HAT menus, recording, animations and sequences are not ported.

## Tablet app

Open `http://<pi-address>/` on a tablet at the piano. On an iPad, *Share → Add
to Home Screen* gives a full-screen app.

**Android** only installs apps from secure (HTTPS) sites, so plv also serves
`https://<pi-address>/` with a certificate from its own small certificate
authority, created on first start in `/var/lib/plv`. Do this once per tablet:

1. Settings → *Install as app* → **Download certificate** (`/ca.crt`).
2. Android settings → Security → Encryption & credentials → Install a
   certificate → **CA certificate**, and pick the file.
3. Open `https://<pi-address>/` in Chrome → menu → **Install app**.

The CA never changes. If the Pi's IP or hostname changes, plv issues a new
server certificate from the same CA at the next start, so the tablet keeps
trusting it.

* **Library**: every `.mid` in the songs folder, with search and upload from
  the tablet (max 1 MB per file, checked to be valid MIDI before it's saved).
* **Play**: falling notes (**Waterfall**) or a scrolling grand staff
  (**Sheet**: treble and bass clefs, note values from whole to sixteenth
  including dots, chords on one shared stem with standard stem direction,
  seconds and accidentals offset so they don't collide, ledger lines, and bar
  lines from the file's time signature) above an 88-key keyboard, coloured per hand. Switch in the
  player or under Settings; the choice is saved on the Pi and every open
  tablet follows. Both views are drawn entirely in the browser. With *Show
  note names* on, letters appear inside the sheet note heads, at the bottom of
  falling notes, and on the keys to play. MIDI files have no rests, so none
  are drawn. Keys
  you press light up green, and in wait mode the key the song expects is
  highlighted.
  * **Wait for me**: the song stops at each chord of your hand until you play
    it. Each chord key needs a fresh press made at most 250 ms before its
    note. Short (staccato) and slightly early presses count, holding keys
    down can't skip ahead, and one press never counts for two chords. After
    3 s of waiting, the keys you still need blink fast. The piano plays the
    other hand.
  * **Play along**: same, at tempo, without waiting.
  * **Listen**: the piano plays the whole song, including the song's sustain
    pedal, while the LEDs follow. (In the practice modes the pedal stays
    yours.) Pausing, seeking and stopping release the pedal.
  * Hand (left / both / right), tempo 25–150%, A–B loop, and a **Tracks**
    panel. Each track is *Right hand*, *Left hand*, *Both hands* (split at
    middle C) or *Ignore* (hidden, silent, no LEDs). At most one track plays
    each hand: picking a hand for one track ignores whichever track had it.
    By default the highest track is the right hand, the lowest the left hand,
    and any others are ignored. A single track, or one holding 85% or more of
    the notes, is *Both hands*, and hand reach doesn't apply to those guessed
    hands.
* **Hand reach** (Settings, default 16 semitones = C3 to E4): in chords wider
  than your reach you only play the notes near the melody note (top for the
  right hand, bottom for the left). The piano plays the rest. They show dashed
  in the falling notes.
* **Settings**: brightness, key colour, backlight, hand colours, fades, strip
  geometry and MIDI port. They apply immediately and are saved on the Pi.

On the strip, keys you should play glow in your hand's colour (250 ms early),
notes the piano plays glow slightly dimmer, and keys you press show the key
colour. While practising, a press turns **green** if it's a key the song
expects and **red** if it isn't, both on the strip and on the tablet. The
sustain pedal is ignored, so LEDs follow your keys only. Hints are
always at least twice as bright as the backlight. In wait mode, the next
notes after the ones to play now (one time slice, either hand, like the
original's "future notes") glow faintly in their hand colour; a key that's
only previewed doesn't count as "right" yet. Pausing clears the song from
the strip. The backlight turns
off after 2 minutes with no key pressed and no song playing, and comes back
on the next key press. A key struck again while
still lit dips for 40 ms, so repeated notes stay distinct.

## How it works

```
 piano ─► rtpmidid/USB ─► midi.rs ─► NoteState (atomics) ─┬─► render.rs ─► LED driver ─► WS2812B
                                                          ├─► player.rs (wait-mode check)
                                                          └─► web.rs ─► tablet (pressed keys)
 tablet ─► web.rs (HTTP) ─► player.rs ─► LED hints + MIDI out ─► piano
```

The player runs **on the Pi**, so playback timing, LED hints and the wait-mode
key checks don't depend on WiFi or the tablet staying awake. The tablet draws
the falling notes from a 20 Hz state stream, smoothed locally.

| Module | Job |
|---|---|
| `midi.rs` | `midir` input (note on/off and all-notes-off, decoded in place; the sustain pedal is ignored) and output to the instrument. |
| `notes.rs` | `NoteState`: fixed arrays of atomics for held keys, latched strikes and player hints. No locks, no heap. |
| `keymap.rs` | The original `get_note_position()` (density, shift, reverse, cumulative note offsets), resolved into a `[u16; 128]` table. |
| `render.rs` | Per-key envelopes, re-strike dip, hint and backlight layers, gamma and brightness table, current limiter. |
| `player.rs` | MIDI file parsing (`midly`), hand assignment, hand-reach filter, the three modes, loop, tempo. |
| `settings.rs` | Settings changeable at runtime, validated, saved as JSON (write-then-rename). |
| `web.rs` | `tiny_http` server (HTTP and HTTPS) with the embedded `ui/` files, a JSON API and a push-only WebSocket. |
| `tls.rs` | Local certificate authority and server certificate for HTTPS (`rcgen`). |
| `hal/` | LED drivers: PWM (GPIO18), SPI (GPIO10), terminal preview, null. |

### Memory and latency

* The MIDI callback and the render loop never allocate or lock. The render
  thread only `try_lock`s the settings when their generation counter changes.
* Rust has no garbage collector, so there are no pauses.
* A note-on wakes the render thread at once, so key-to-light latency is about
  one strip transfer (~6 ms for 176 LEDs) instead of a full frame.
* Measured on the Zero 2 W with the app running: ~3 MB RSS, ~1% CPU.

### HTTP API

| Method | Path | |
|---|---|---|
| GET | `/api/songs` | `{songs: [{name, size}]}` |
| POST | `/api/songs?name=<file.mid>` | raw `.mid` body |
| GET/PUT | `/api/settings` | whole settings object |
| GET | `/api/ports` | MIDI inputs |
| GET/POST | `/api/player` | status / `{song?, playing?, mode?, hand?, tempo?, seek?, loop_ab?, track_hands?, reach?}` |
| GET | `/api/player/song` | loaded song: tracks + notes `[t, dur, key, hand, out_of_reach]` |
| GET | `/ws` | WebSocket: player status + held keys, 20×/s |

There's no authentication, the same as the original: keep it on your home network.

## LED drivers and wiring

| `--driver` | Data pin | Needs | Notes |
|---|---|---|---|
| `pwm` | **GPIO18, physical pin 12** (same as the original) | root, `--features pwm` | Uses the same `rpi_ws281x` C library as the original. Works with the Waveshare LCD HAT. |
| `spi` | GPIO10 (SPI0 MOSI), physical pin 19 | group `spi` | Pure Rust, no root. Clashes with the LCD HAT, which uses SPI0. |
| `terminal` / `null` | – | – | For development and benchmarking. |

When built with `--features pwm` the default driver is `pwm`; otherwise it's
`spi`. A 3.3 V → 5 V level shifter (e.g. 74AHCT125) on DIN is recommended, just
as with the original.

**PWM** (`--gpio 18 --dma 10` by default): needs root (`/dev/mem` and the
VideoCore mailbox). Never use DMA channel 5 on a Zero 2 W, because the SD card
uses it.

**SPI**: set `dtparam=spi=on`, `core_freq=250` and `core_freq_min=250` in
`/boot/firmware/config.txt`. With more than ~440 LEDs, also add
`spidev.bufsiz=32768` to `cmdline.txt`.

## Network MIDI (rtpmidid) and WiFi power saving

If the piano reaches the Pi over WiFi (rtpMIDI from a PC to `rtpmidid`), turn
off WiFi power saving. Otherwise incoming MIDI gets held up for seconds and
then arrives in bursts, which looks like frozen LEDs. To turn it off now:

```bash
sudo iw dev wlan0 set power_save off
```

To keep it off across reboots, through NetworkManager:

```bash
sudo nmcli connection modify "$(nmcli -t -f NAME connection show --active | head -1)" 802-11-wireless.powersave 2
```

## Build

Cross-compile on a PC; the Zero 2 W has too little RAM and disk for a Rust
toolchain. For 32-bit Raspberry Pi OS (Bookworm, armv7):

```bash
docker build -t plv-armv7 -f docker/Dockerfile.armv7 docker
```

```bash
docker run --rm -v "$PWD:/src" plv-armv7 cargo build --release --features pwm --target armv7-unknown-linux-gnueabihf
```

The binary ends up at `target/armv7-unknown-linux-gnueabihf/release/plv`. The UI
is embedded in it, so it's the only file to copy. Leave out `--features pwm`
for an SPI-only build with no C dependencies.

## Install as a service (replacing the Python visualizer)

```bash
sudo install -m 755 plv /usr/local/bin/plv
```

```bash
sudo install -m 644 systemd/plv.service /etc/systemd/system/plv.service
```

```bash
sudo systemctl disable --now visualizer.service
```

```bash
sudo systemctl daemon-reload && sudo systemctl enable --now plv.service
```

The unit serves the app on port 80 and HTTPS on 443 (the original's PHP add-on keeps 8080) and
reads songs from `/home/Piano-LED-Visualizer/Songs`. Settings live in
`/var/lib/plv/settings.json`; on the first start they're seeded from the
command-line flags and written there. The unit declares
`Conflicts=visualizer.service`, so the two never drive GPIO18 at the same time.

To switch back to the Python version:

```bash
sudo systemctl disable --now plv.service && sudo systemctl enable --now visualizer.service
```

## Run by hand

```bash
plv --list-ports
```

```bash
plv --port CHESTNUT --http 0.0.0.0:8080 --songs ./songs --settings ./settings.json
```

```bash
plv --demo
```

`--demo` plays a scale without a keyboard, for checking wiring and `--shift`.
On a desktop, `cargo run -- --driver terminal --songs <dir>` shows the strip in
the terminal and serves the app on `http://localhost:8080/`. `plv --help`
lists all options. Once a settings file exists, it wins over strip and colour
flags.

### Mapping compatibility

The mapping matches the original's `get_note_position()` exactly (including
`led_count - pos` when reversed), so shift and offset values calibrated in the
Python version carry over. The one difference: notes that fall off the strip
stay dark instead of piling onto LED 0.

## Tests

```bash
cargo test
```

Tests cover the mapping against the original formula, the SPI bit encoding,
MIDI decoding, envelopes, re-strike and backlight layering, the power limiter,
MIDI file parsing with tempo changes, hand assignment, the hand-reach filter,
the wait / play-along / listen modes, loops, settings validation and
persistence, and the web API's path handling.
