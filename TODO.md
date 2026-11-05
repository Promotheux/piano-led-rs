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
- [x] **Native RTP-MIDI**: built-in AppleMIDI/RTP-MIDI listener on plain threads and UDP (no Tokio), announced via avahi; replaces rtpmidid.
- [x] **Wi-Fi Fallback Hotspot (AP Mode)**: `PianoLED-Setup` via NetworkManager plus a watchdog timer (`install.sh`).
- [x] **Wi-Fi setup page in the app**: pick a network and enter its password while on the hotspot, for headless reconfiguration.
- [x] **Provisioning script**: `install.sh` (binary, service, songs, power save, fallback hotspot; `--dry-run`).
- [x] **Flashable image**: `build_image.sh` builds it locally from a downloaded Raspberry Pi OS Lite image.

## Phase 2: Practice features & learning engine

- [x] **Perfect pitch trainer mode** (Train tab):
  - Note, interval (up to an octave) and triad drills, played back on the piano.
  - Strict blind test, or a reference C4 before each question.
  - Instant LED feedback: green for correct; red, with the right keys revealed
    in blue, for incorrect.
  - Streak, best, accuracy score and an auto range that grows with the streak.
- [x] **Improv mode** (Improv tab): the LEDs show which notes sound good right
      now, for improvising with the right hand.
  - Chords from you (left hand below a split point, recognised live) or from a
    backing progression the piano plays (e.g. I–V–vi–IV, ii–V–I, 12-bar blues)
    in a chosen key and tempo.
  - Current chord's tones bright (safe to land on), the rest of the scale dim
    (passing notes), everything else dark; or a pentatonic-only setting.
- [x] **Chord progressions** (Apps): I–IV–V–I, ii–V–I, 12-bar blues and more,
      read as Roman numerals or by ear, in a set or random key; any voicing
      counts, scored on root and quality.
- [x] **Smart loop practice**: "Speed up" raises the tempo 5% after each A–B
      pass without a wrong note, up to 100%.
- [x] **Metronome and count-in**: clicks from the tablet on the song's beats;
      count-in replays the bar before silently.
- [x] **Session scorecards & statistics**: right/wrong notes and waiting time
      per run; best tempo and accuracy per song in the Library.
- [x] **Record your playing** to a MIDI file in the library (keys and pedal).

## Phase 3: Sheet music & frontend polish

- [x] **Key signature detection**: from the MIDI key-signature event, or
      guessed from the notes; spelling, the signature on the staff, and
      accidentals per bar (with naturals) follow it.
- [x] **Staff layout**: beams for eighths and sixteenths, measure numbers,
      8va/8vb lines for very high/low notes.
- [x] **Split `ui/app.js`** into ES modules in `ui/js/` (still no build step).

## Phase 4: DevOps & hardware integrations

- [ ] **GitHub Actions**: run `cargo test` and cross-compile the armv7 binary on
      every push; attach the binary to releases.
- [ ] **Idle LED animations** (breathing, rainbow) when nothing is playing.
