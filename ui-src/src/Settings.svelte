<script>
  // Settings: connection health, Wi-Fi, LEDs, strip, practice, system, install.
  // Every change saves itself; styles come from ui/app.css.
  import { api, ICON, post, toast } from '/js/util.js';
  import { fullName, KEY_NAMES } from '/js/music.js';
  import { cfg, scheduleSave } from './settings.svelte.js';

  const HEALTH = [
    ['link', 'Tablet link'],
    ['in', 'MIDI in'],
    ['out', 'MIDI out'],
    ['fps', 'LED frame rate'],
    ['ws', 'Tablets connected'],
    ['rtp', 'Network MIDI'],
  ];
  const SPLITS = Array.from({ length: 49 }, (_, i) => 36 + i); // C2 … C6
  const KEYS = Array.from({ length: 88 }, (_, i) => 21 + i); // A0 … C8

  // Multicolor ranges: add one after the last (or at middle C when the last
  // already reaches the top key), or remove one, then save.
  function addRange() {
    const ranges = cfg.form.multicolor;
    const end = ranges.at(-1)?.[1] ?? 20;
    const from = end < 108 ? end + 1 : 60;
    ranges.push([from, Math.min(108, from + 11), '#ffffff']);
    scheduleSave(600);
  }
  function removeRange(i) {
    cfg.form.multicolor.splice(i, 1);
    scheduleSave(600);
  }

  // A setting changed (Wi-Fi fields have no name: they aren't settings).
  function changed(e) {
    if (e.target.name) scheduleSave(e.target.type === 'range' ? 150 : 600);
  }

  // Wi-Fi
  let wifiNow = $state('…');
  let networks = $state(null); // null: not scanned yet
  let scanning = $state(false);
  let ssid = $state('');
  let password = $state('');
  let wifiStatus = $state('');
  let passwordField;

  async function loadWifiCurrent() {
    try {
      const c = await api('/api/wifi/current');
      wifiNow = !c.ssid ? 'no network' : c.hotspot ? `its own hotspot (${c.ssid})` : c.ssid;
    } catch {
      wifiNow = 'unknown';
    }
  }
  loadWifiCurrent();

  async function scan() {
    loadWifiCurrent();
    scanning = true;
    try {
      networks = (await api('/api/wifi')).networks;
    } catch (e) {
      networks = null;
      toast(e.message, true);
    }
    scanning = false;
  }

  function pick(n) {
    ssid = n.ssid;
    passwordField.focus();
  }

  async function connect() {
    const name = ssid.trim();
    if (!name) return toast('Pick or type a network name', true);
    if (!confirm(`Switch the Pi to "${name}"? This page loses its connection while the Pi switches.`)) return;
    try {
      await post('/api/wifi', { ssid: name, password });
      password = '';
      wifiStatus =
        `Switching to "${name}". Join that network with this tablet too, then find the Pi in your router ` +
        `(it's called pianoledvisualizer) or try http://pianoledvisualizer.local/. ` +
        `If the password was wrong, the PianoLED-Setup hotspot comes back within a minute.`;
    } catch (e) {
      toast(e.message, true);
    }
  }

  async function system(path, question, done) {
    if (!confirm(question)) return;
    try {
      await post(path, {});
      toast(done);
    } catch (e) {
      toast(e.message, true);
    }
  }

  // Chrome on Android only installs over HTTPS (with the Pi's certificate).
  const httpsLink = `https://${location.hostname}${location.port === '8080' ? ':8443' : ''}/`;
  const installState = window.isSecureContext
    ? 'This page is secure, so Chrome can install it (menu → Install app).'
    : 'This page is plain http, so Chrome can only make a shortcut. Follow the steps below.';
</script>

<form class="settings" autocomplete="off" oninput={changed} onsubmit={(e) => e.preventDefault()}>
  <h2>Connection</h2>
  <div class="group">
    {#each HEALTH as [id, label] (id)}
      <div class="item">
        <span class="lbl">{label}</span><span class={['val', { bad: cfg.health[id]?.bad }]}>{cfg.health[id]?.text ?? '–'}</span>
      </div>
    {/each}
    {#if cfg.form}
      <label class="item"
        ><span class="lbl"
          >MIDI port (USB)
          <small>Network MIDI (RTP-MIDI, "Piano LED") is always on as well.</small></span
        >
        <select name="midi_port" bind:value={cfg.form.midi_port}>
          <option value="">Automatic</option>
          {#each cfg.ports as p (p)}<option value={p}>{p}</option>{/each}
        </select></label
      >
    {/if}
  </div>

  <h2>Wi-Fi</h2>
  <div class="group">
    <div class="item"><span class="lbl">Connected to</span><b class="val">{wifiNow}</b></div>
    <button type="button" class="item action" onclick={scan}>Scan for networks</button>
    {#if scanning || networks}
      <ul class="wifilist">
        {#if scanning}
          <li class="muted">Scanning…</li>
        {:else if networks.length === 0}
          <li class="muted">No networks found. On its own hotspot the Pi often can't scan: type the name below.</li>
        {:else}
          {#each networks as n (n.ssid)}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
            <li onclick={() => pick(n)}>
              <span>{n.ssid}{n.active ? ' ✓' : ''}</span><span class="meta">{n.signal}%{n.secure ? ' 🔒' : ''}</span>
            </li>
          {/each}
        {/if}
      </ul>
    {/if}
    <label class="item"
      ><span class="lbl">Network name</span>
      <input type="text" maxlength="32" autocomplete="off" autocapitalize="off" spellcheck="false" bind:value={ssid} /></label
    >
    <label class="item"
      ><span class="lbl">Password <small>Leave empty for an open network.</small></span>
      <input type="password" maxlength="63" autocomplete="new-password" bind:value={password} bind:this={passwordField} /></label
    >
    <button type="button" class="item action" onclick={connect}>Connect the Pi</button>
  </div>
  <small class="foot">{wifiStatus}</small>

  {#if cfg.form}
    {@const f = cfg.form}
    <h2>Appearance &amp; LEDs</h2>
    <div class="group">
      <label class="item"><span class="lbl">Brightness</span><input name="brightness" type="range" min="0" max="255" bind:value={f.brightness} /></label>
      <label class="item"
        ><span class="lbl">Key colour</span>
        <span class="ctl"
          ><select name="colormode" bind:value={f.colormode}>
            <option value="rainbow">Rainbow</option>
            <option value="spiral">Note colours (C pink … B red)</option>
            <option value="gradient">Gradient</option>
            <option value="scale">Scale</option>
            <option value="multicolor">Multicolor (key ranges)</option>
            <option value="velocity">Velocity rainbow</option>
            <option value="single">Single colour</option>
          </select>
          {#if f.colormode === 'single'}
            <input name="color" type="color" aria-label="Single colour" bind:value={f.single} />
          {/if}</span
        ></label
      >
      {#if f.colormode === 'rainbow'}
        <label class="item"
          ><span class="lbl">Rainbow size <small>{f.rainbow_scale}% of the colour circle across the keys</small></span>
          <input name="rainbow_scale" type="range" min="10" max="400" step="10" bind:value={f.rainbow_scale} /></label
        >
        <label class="item"
          ><span class="lbl"
            >Drift <small
              >{f.rainbow_speed === 0 ? 'Still' : `${Math.abs(f.rainbow_speed)}° a second, ${f.rainbow_speed < 0 ? 'to the left' : 'to the right'}`}</small
            ></span
          >
          <input name="rainbow_speed" type="range" min="-120" max="120" step="5" bind:value={f.rainbow_speed} /></label
        >
      {:else if f.colormode === 'gradient'}
        <div class="item">
          <span class="lbl">Gradient <small>From the lowest key to the highest</small></span>
          <span class="ctl">
            <input name="gradient_colors" type="color" aria-label="Lowest key colour" bind:value={f.gradient_colors[0]} />
            <input name="gradient_colors" type="color" aria-label="Highest key colour" bind:value={f.gradient_colors[1]} />
          </span>
        </div>
      {:else if f.colormode === 'scale'}
        <label class="item"
          ><span class="lbl">Scale <small>Notes of this major scale in one colour, the rest in the other. C: white and black keys.</small></span>
          <select name="scale_key" bind:value={f.scale_key}>
            {#each KEY_NAMES as n, i (i)}<option value={i}>{n} major</option>{/each}
          </select></label
        >
        <div class="item">
          <span class="lbl">Scale colours <small>In the scale, out of it</small></span>
          <span class="ctl">
            <input name="scale_colors" type="color" aria-label="In-scale colour" bind:value={f.scale_colors[0]} />
            <input name="scale_colors" type="color" aria-label="Out-of-scale colour" bind:value={f.scale_colors[1]} />
          </span>
        </div>
      {:else if f.colormode === 'multicolor'}
        {#each f.multicolor as range, i (i)}
          <div class="item">
            <span class="lbl">Range {i + 1}</span>
            <span class="ctl">
              <select
                name="multicolor"
                aria-label="Range {i + 1} from"
                bind:value={range[0]}
                onchange={() => (range[1] = Math.max(range[1], range[0]))}
              >
                {#each KEYS as k (k)}<option value={k}>{fullName(k)}</option>{/each}
              </select>
              <select name="multicolor" aria-label="Range {i + 1} to" bind:value={range[1]}>
                {#each KEYS as k (k)}<option value={k} disabled={k < range[0]}>{fullName(k)}</option>{/each}
              </select>
              <input name="multicolor" type="color" aria-label="Range {i + 1} colour" bind:value={range[2]} />
              <!-- A fixed SVG string from util.js, never user data. -->
              <button type="button" class="icon-btn" aria-label="Remove range {i + 1}" onclick={() => removeRange(i)}>{@html ICON.trash}</button>
            </span>
          </div>
        {/each}
        {#if f.multicolor.length < 16}
          <button type="button" class="item action" onclick={addRange}>Add range <small>(keys between ranges blend)</small></button>
        {/if}
      {:else if f.colormode === 'velocity'}
        <div class="item text"><small>Each key's colour follows how hard you strike it: soft blue, through green and yellow, to hard red.</small></div>
      {/if}
      <label class="item"
        ><span class="lbl">Backlight <small>Constant glow along the keys. Slide left to turn it off.</small></span>
        <span class="ctl"
          ><input name="backlight_brightness" type="range" min="0" max="255" aria-label="Backlight brightness" bind:value={f.backlight_brightness} />
          <input name="backlight_color" type="color" aria-label="Backlight colour" bind:value={f.backlight_color} /></span
        ></label
      >
      <label class="item"><span class="lbl">Right hand hint</span><input name="right_color" type="color" bind:value={f.right_color} /></label>
      <label class="item"><span class="lbl">Left hand hint</span><input name="left_color" type="color" bind:value={f.left_color} /></label>
      <label class="item"
        ><span class="lbl">Fade <small>Milliseconds</small></span><input name="fade_ms" type="number" min="0" max="20000" step="50" bind:value={f.fade_ms} /></label
      >
      <label class="item"
        ><span class="lbl">Brightness follows velocity</span><input name="velocity" type="checkbox" class="switch" bind:checked={f.velocity} /></label
      >
    </div>

    <h2>LED strip</h2>
    <div class="group">
      <label class="item"><span class="lbl">LEDs per metre</span><input name="leds_per_meter" type="number" min="1" max="500" bind:value={f.leds_per_meter} /></label>
      <label class="item"><span class="lbl">Shift</span><input name="shift" type="number" min="-500" max="500" bind:value={f.shift} /></label>
      <label class="item"
        ><span class="lbl">Reversed <small>LED 0 at the treble end</small></span><input name="reverse" type="checkbox" class="switch" bind:checked={f.reverse} /></label
      >
      <label class="item"><span class="lbl">LEDs per key</span><input name="span" type="number" min="1" max="16" bind:value={f.span} /></label>
      <label class="item"
        ><span class="lbl">Note offsets <small>note:LEDs, comma separated</small></span><input name="note_offsets" type="text" placeholder="92:2, 55:1" bind:value={f.offsets} /></label
      >
      <label class="item"><span class="lbl">Current limit <small>mA, 0 = off</small></span><input name="max_ma" type="number" min="0" step="100" bind:value={f.max_ma} /></label>
      <label class="item"
        ><span class="lbl">LED count <small>Applies after a restart.</small></span><input name="led_count" type="number" min="1" max="2000" bind:value={f.led_count} /></label
      >
    </div>

    <h2>Practice &amp; Improv</h2>
    <div class="group">
      <label class="item"
        ><span class="lbl">Player view</span>
        <select name="view_mode" bind:value={f.view_mode}>
          <option value="waterfall">Waterfall (falling notes)</option>
          <option value="sheet">Sheet music (staff)</option>
        </select></label
      >
      <label class="item"><span class="lbl">Show note names</span><input name="note_names" type="checkbox" class="switch" bind:checked={f.note_names} /></label>
      <label class="item"
        ><span class="lbl"
          >Hand reach <small>Semitones. 16 = C3 to E4. Wider chords: the piano plays the notes you can't reach. 0 = off.</small></span
        >
        <input name="hand_reach" type="number" min="0" max="48" bind:value={f.hand_reach} /></label
      >
      <label class="item"
        ><span class="lbl">Improv lights</span>
        <select name="improv_lights" bind:value={f.improv_lights}>
          <option value="scale">Chord bright, scale dim</option>
          <option value="pentatonic">Pentatonic only</option>
          <option value="chord">Chord tones only</option>
        </select></label
      >
      <label class="item"
        ><span class="lbl">Improv split <small>Chords are read below this key; the LEDs guide the keys from here up.</small></span>
        <select name="improv_split" bind:value={f.improv_split}>
          {#each SPLITS as k (k)}<option value={k}>{fullName(k)}</option>{/each}
        </select></label
      >
    </div>
  {/if}

  <h2>System</h2>
  <div class="group">
    <button
      type="button"
      class="item action"
      onclick={() => system('/api/system/restart', 'Restart plv? The LEDs and this page pause for a few seconds.', 'Restarting plv…')}
      >Restart plv</button
    >
    <button
      type="button"
      class="item action danger"
      onclick={() =>
        system('/api/system/reboot', 'Reboot the Raspberry Pi? It takes about a minute to come back.', 'Rebooting the Pi… this page reconnects by itself.')}
      >Reboot Pi</button
    >
  </div>

  <h2>Install as app (Android)</h2>
  <div class="group">
    <div class="item text">
      <small>{installState}</small>
      <ol class="steps">
        <li><a class="btn" href="/ca.crt" download>Download certificate</a></li>
        <li>
          Android settings → Security → More security settings → Encryption &amp; credentials → Install a certificate →
          <b>CA certificate</b>, and pick the downloaded file.
        </li>
        <li>Open <a href={httpsLink}>{httpsLink}</a> in Chrome, then menu → <b>Install app</b>.</li>
      </ol>
    </div>
  </div>
  <small class="foot">The certificate is made by your Pi and only works for it. On an iPad, Share → Add to Home Screen works without this.</small>
</form>
