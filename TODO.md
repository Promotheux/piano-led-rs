# Roadmap

Prioritized: work top to bottom. Tick items off as they land.

## Phase 1: Reliability & diagnostics (high priority)

- [x] **Stuck-note safety net**: auto-release a held key after a timeout without
      a note-off (lost packets over network MIDI), so its LED can't stay lit.
- [x] **Connection health in the UI/API**: MIDI port in use, WebSocket state,
      LED frame rate, so a freeze can be diagnosed from the tablet.
- [x] **Document the Wi-Fi power-save fix** (`iw dev wlan0 set power_save off`,
      plus the permanent NetworkManager setting): see README, "Network MIDI".

## Phase 1.5: System Provisioning, RTP-MIDI & Network Fallback
- [ ] **Native Rust RTP-MIDI integration**: replace external system daemons by implementing AppleMIDI/RTP-MIDI directly in Tokio using `rtpmidi` with automatic mDNS advertisement.
- [ ] **Wi-Fi Fallback Hotspot (AP Mode)**: configure NetworkManager / auto-watchdog script to spawn an AP hotspot (`PianoLED-Setup`) when the primary Wi-Fi connection drops, enabling headless network reconfiguration from a phone or tablet.
- [ ] **Flashable RPi Image & Provisioning Script**: create a single-command installer and automated build pipeline to flash a clean Raspberry Pi OS Lite image with pre-configured binaries, `systemd` services, power-save overrides, and fallback profiles.

## Phase 2: Practice features & learning engine

- [ ] **Perfect pitch trainer mode**:
  - Single note, chord quality and interval identification drills, answered on
    the piano (MIDI input).
  - Strict mode (blind test) or a reference drone / relative aid.
  - Instant LED feedback: green for correct; red, with the right key revealed
    in blue, for incorrect.
  - Streak tracking, accuracy score and adaptive octave range.
- [ ] **Harmonic ear training & chord progressions**: the app prompts a Roman
      numeral progression (e.g. ii–V–I in G major) by sound or symbol; you play
      the chords in any voicing, and it scores the harmony (right chord
      function and quality) rather than exact notes.
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
