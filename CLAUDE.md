# plv — Piano LED Visualizer (Rust)

Rewrite of onlaj/Piano-LED-Visualizer for a Raspberry Pi Zero 2 W: MIDI in → WS2812B strip, plus a tablet practice app served from the same binary.

## Commands

- Test: `cargo test` (unit tests live in `#[cfg(test)]` modules inside each file)
- Run on a desktop: `cargo run -- --driver terminal --songs <dir>` → app on http://localhost:8080/
- Pi build (armv7, via Docker): see README "Build". `--features pwm` pulls in the C `rpi_ws281x` lib; leave it out for SPI-only.

## Rules

- The MIDI callback and the render loop must never allocate or lock (`notes.rs` uses atomics; render only `try_lock`s settings on a generation change).
- `keymap.rs` must match the original's `get_note_position()` exactly, so calibrated shift/offset values carry over.
- `ui/` is embedded into the binary. Most of it is plain ES modules (`ui/js/`, no build). The Library, Settings and the Apps are Svelte: source in `ui-src/`, built with `cd ui-src && npm run build` into `ui/dist/app.js`, which is committed so the Rust/Docker build needs no Node. Rebuild and commit `ui/dist/` after changing `ui-src/`.
- Keep it small: the target has ~512 MB RAM and runs at ~3 MB RSS. Don't add heavy dependencies.
- `rppal`/`rs_ws281x` only build on Linux; Windows dev uses the terminal/null drivers.
- Commit subjects: short, imperative, sentence case, no prefix (e.g. "Smooth the tablet's position in wait mode").

## Read when needed

- `README.md` — architecture diagram, module table, HTTP API, wiring, service install, rtpmidid/WiFi notes.
- `TODO.md` — roadmap by phase; check it before planning new features.
