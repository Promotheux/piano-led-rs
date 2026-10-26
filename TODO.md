# Roadmap

Prioritized: work top to bottom. Tick items off as they land.

## Phase 1: Reliability & diagnostics (high priority)

- [ ] **Stuck-note safety net**: auto-release a held key after a timeout without
      a note-off (lost packets over network MIDI), so its LED can't stay lit.
- [ ] **Connection health in the UI/API**: MIDI port in use, WebSocket state,
      LED frame rate, so a freeze can be diagnosed from the tablet.
- [x] **Document the Wi-Fi power-save fix** (`iw dev wlan0 set power_save off`,
      plus the permanent NetworkManager setting): see README, "Network MIDI".

## Phase 2: Practice features & learning engine

- [ ] **Smart loop practice**: A–B loop that raises the tempo automatically after
      each clean pass (e.g. 60% → 100% in 5% steps).
- [ ] **Metronome and count-in** for Play-along mode.
- [ ] **Session scorecards & statistics**: right/wrong notes, waiting time, best
      tempo per song.
- [ ] **Record your playing** to a MIDI file in the library.

## Phase 3: Sheet music & frontend polish

- [ ] **Key signature detection** (MIDI key-signature event) for correct
      sharp/flat spelling instead of the fixed C♯/E♭/F♯/A♭/B♭ scheme.
- [ ] **Staff layout**: beams for eighths and sixteenths, measure numbers, 8va
      indicators for very high/low notes.
- [ ] **Split `ui/app.js`** into ES modules (still no build step).

## Phase 4: DevOps & hardware integrations

- [ ] **GitHub Actions**: run `cargo test` and cross-compile the armv7 binary on
      every push; attach the binary to releases.
- [ ] **Idle LED animations** (breathing, rainbow) when nothing is playing.
