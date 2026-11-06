# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Piano players who install plv on their own Raspberry Pi and their own digital piano (open-source users), with an LED strip above the keys and a tablet on the music stand. The author builds and tests it on a Yamaha Portable Grand at home, but the product is for anyone with that setup, not one person.

They use it at the piano: the tablet sits at arm's length on the music stand, the room is often dim (evening practice), and their hands are on the keys most of the time. The tablet is in **landscape**, and the app must work best that way; portrait and phones must still work.

## Product Purpose

plv lights the keys to play on an LED strip and gives a tablet app at the piano for choosing and practising songs. Its main job is **learning songs**: wait mode (the song waits until you play the lit keys), playing one hand while the piano plays the other, A–B loops that speed up, a metronome and count-in, and scorecards per song.

Secondary features: the Apps (pitch trainer, improv, chord progressions), recording what you play, and a library with titles, artists and difficulty ratings.

Success: a player picks a song on the tablet, and learns it by following the lights, without fighting the interface while their hands are on the keys.

## Positioning

A rewrite of onlaj/Piano-LED-Visualizer as one small Rust binary for a Raspberry Pi Zero 2 W. The player runs on the Pi, not in the browser: MIDI timing stays accurate, playback survives the tablet's screen locking, and wait mode checks your keys without a network round trip. The tablet app is served by the same binary and installs as a full-screen app. MIDI comes in over USB or the built-in RTP-MIDI (network MIDI), and a fallback Wi-Fi hotspot lets you set up the network from the tablet.

## Operating Context

- Hardware: Raspberry Pi Zero 2 W (~512 MB RAM), a WS2812B LED strip above the keys (PWM or SPI), a digital piano over USB MIDI or RTP-MIDI from a PC.
- Install: a flashable Raspberry Pi OS image (`build_image.sh`) or `install.sh` on an existing Pi; first-time Wi-Fi setup through the `PianoLED-Setup` hotspot.
- The tablet (Android or iPad) opens the app over the home network; Chrome on Android can install it over HTTPS.
- Songs are the user's own MIDI files, uploaded or recorded; each may have a sidecar JSON with title, artist and difficulty.
- Several tablets can be open at once; settings and state are pushed to all of them live.

## Capabilities and Constraints

- Modes: Wait for me, Play along (the piano plays the other hand), Listen. Hands: left, right, both. Tempo, A–B loop with automatic speed-up, count-in, metronome (clicks from the tablet).
- Views: falling-notes waterfall and scrolling sheet music (key signatures, beams, 8va), with the keyboard below.
- LED feedback: hand colours, previews, green/red judging, dim green for chord keys already played, a backlight, note colours per pitch class.
- Library: search, sort (title, artist, difficulty), difficulty filter, upload, record, edit title/artist/difficulty, rename, delete.
- Apps: pitch trainer, improv, chord progressions.
- Settings: LEDs and strip geometry, colours, MIDI, Wi-Fi, practice options, restart/reboot.
- Technical constraints: runs in ~5 MB of memory; no heavy dependencies; the UI is embedded in the binary: plain ES modules, plus Svelte for the Library, Settings and the Apps, built on the developer's machine (never on the Pi); the MIDI callback and render loop never allocate or lock; LAN only, no authentication (same as the original).
- Terminology: "wait mode", "play along", "listen", "hand", "loop A–B", "Speed up", "count-in", "Apps", "Library".

## Brand Commitments

- Name in the app: **Piano LED** (the binary and project are `plv`).
- Existing app icon: `ui/icon.svg` (also `ui/icon-192.png`, `ui/icon-512.png`).

## Evidence on Hand

- The working product on real hardware (Pi Zero 2 W, LED strip, Yamaha Portable Grand), and its README (architecture, wiring, API).
- No testimonials, user counts, press or benchmarks against other products exist; future work must not invent them. Songs in the developer's library are personal files, not content the product ships.

## Product Principles

1. **The piano comes first.** Every screen should work at arm's length, with one quick tap, while the hands are on the keys; the LEDs carry the moment-to-moment guidance, the tablet supports it.
2. **Practice is the core loop.** Picking a song and learning it (wait, loop, speed up, score) outranks everything else in placement and polish.
3. **Calm in a dim room.** Nothing should glare or distract from playing; feedback is clear but quiet.
4. **Small and reliable.** It runs on a tiny Pi for hours; features never cost timing, memory or stability.
5. **Respect the player's library.** Songs are their own files; metadata lives beside them and is never destructive.

## Accessibility & Inclusion

- Readable and tappable from the music stand: large text, large tap targets (at least 44 px, more on touch screens), clear focus states.
- Comfortable in a dim room: a dark interface without bright flashes.
- Hands-on-keys use: the most frequent actions (play/pause, restart, loop) are single taps; nothing important hides behind hover, since tablets can't hover.
