// Browser host for the Empire WebAssembly module.
//
// Responsibilities: load the module, size the canvas, forward input, blit the
// framebuffer, and carry out the module's requests (storage, sounds, file
// download/upload). All game logic lives in Rust.

const canvas = document.getElementById('screen');
const ctx = canvas.getContext('2d', { alpha: false });
const statusEl = document.getElementById('status');
const fileInput = document.getElementById('file');
const enc = new TextEncoder();
const dec = new TextDecoder();

let ex = null;

function setStatus(text) {
  statusEl.textContent = text;
}

function mem() {
  return new Uint8Array(ex.memory.buffer);
}

// Copy bytes into module memory, call fn(ptr, len), then free.
function withBytes(bytes, fn) {
  const len = bytes.length;
  const ptr = ex.empire_alloc(len);
  mem().set(bytes, ptr);
  try {
    return fn(ptr, len);
  } finally {
    ex.empire_free(ptr, len);
  }
}

function withStr(s, fn) {
  return withBytes(enc.encode(s), fn);
}

function withStr2(a, b, fn) {
  return withStr(a, (pa, la) => withStr(b, (pb, lb) => fn(pa, la, pb, lb)));
}

async function loadModule() {
  const resp = await fetch('empire.wasm', { cache: 'no-cache' });
  if (!resp.ok) throw new Error(`empire.wasm: HTTP ${resp.status} (run scripts/build-web.sh)`);
  const bytes = await resp.arrayBuffer();
  const imports = {
    env: {
      host_log(ptr, len) {
        console.error(dec.decode(new Uint8Array(ex.memory.buffer, ptr, len)));
      },
    },
  };
  const { instance } = await WebAssembly.instantiate(bytes, imports);
  ex = instance.exports;
  ex.empire_init();
}

// Settings, maps and the commanders log live in localStorage under "empire.".
// Saved games live in IndexedDB, which has room for many more of them; saves
// left in localStorage by older versions are moved over on startup.
const isSaveKey = (k) => k.startsWith('empire.save.') || k === 'empire.autosave';
let saveDb = null;

function openSaveDb() {
  return new Promise((resolve) => {
    let req;
    try {
      req = indexedDB.open('empire', 1);
    } catch {
      resolve(null);
      return;
    }
    req.onupgradeneeded = () => req.result.createObjectStore('saves');
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => resolve(null);
  });
}

function idb(mode, fn) {
  return new Promise((resolve, reject) => {
    const tx = saveDb.transaction('saves', mode);
    const out = fn(tx.objectStore('saves'));
    tx.oncomplete = () => resolve(out?.result);
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error);
  });
}

async function loadStorage() {
  const entries = new Map();
  for (let i = 0; i < localStorage.length; i++) {
    const key = localStorage.key(i);
    if (key && key.startsWith('empire.')) entries.set(key, localStorage.getItem(key) ?? '');
  }
  saveDb = await openSaveDb();
  if (saveDb) {
    try {
      const keys = await idb('readonly', (s) => s.getAllKeys());
      const values = await idb('readonly', (s) => s.getAll());
      keys.forEach((k, i) => {
        if (!entries.has(k)) entries.set(k, values[i]);
      });
      // Move saves from older versions out of localStorage.
      for (const [k, v] of entries) {
        if (isSaveKey(k) && localStorage.getItem(k) !== null) {
          await idb('readwrite', (s) => s.put(v, k));
          localStorage.removeItem(k);
        }
      }
    } catch (e) {
      console.warn('saved games unavailable', e);
    }
  }
  for (const [key, value] of entries) {
    withStr2(key, value, (a, b, c, d) => ex.empire_storage(a, b, c, d));
  }
}

function store(key, value) {
  if (saveDb && isSaveKey(key)) {
    idb('readwrite', (s) => (value === null ? s.delete(key) : s.put(value, key))).catch((e) => {
      console.error('save failed', e);
      setStatus('COULD NOT SAVE THE GAME - BROWSER STORAGE IS FULL');
    });
    return;
  }
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch (e) {
    console.error('storage failed', e);
    setStatus('BROWSER STORAGE IS FULL - DELETE OLD SAVES');
  }
}

// Optional assets converted from an original Interstel disk.
async function loadOriginalAssets() {
  let manifest;
  try {
    const r = await fetch('assets/original/manifest.json', { cache: 'no-cache' });
    if (!r.ok) return 0;
    manifest = await r.json();
  } catch {
    return 0;
  }
  let loaded = 0;
  for (const name of manifest.files ?? []) {
    try {
      const r = await fetch(`assets/original/${name}`, { cache: 'no-cache' });
      if (!r.ok) continue;
      if (name.endsWith('.img')) {
        const bytes = new Uint8Array(await r.arrayBuffer());
        withStr(name, (np, nl) => withBytes(bytes, (dp, dl) => ex.empire_asset_bytes(np, nl, dp, dl)));
      } else {
        const text = await r.text();
        withStr2(name, text, (a, b, c, d) => ex.empire_asset_text(a, b, c, d));
      }
      loaded++;
    } catch (e) {
      console.warn(name, e);
    }
  }
  return loaded;
}

function resize() {
  ex.empire_set_viewport(window.innerWidth, window.innerHeight, window.devicePixelRatio || 1);
  fitCanvas();
}

// The module decides the canvas resolution and display size: in high-res
// mode one canvas pixel is one device pixel; in CGA mode the 320x200 picture
// is enlarged by a whole number with the CGA pixel shape.
function fitCanvas() {
  const pw = ex.empire_pixel_width();
  const ph = ex.empire_pixel_height();
  if (canvas.width !== pw || canvas.height !== ph) {
    canvas.width = pw;
    canvas.height = ph;
  }
  canvas.style.width = `${ex.empire_display_width()}px`;
  canvas.style.height = `${ex.empire_display_height()}px`;
}

// Mouse positions are passed in layout units.
function toFrame(e) {
  const r = canvas.getBoundingClientRect();
  const x = Math.floor(((e.clientX - r.left) / r.width) * ex.empire_width());
  const y = Math.floor(((e.clientY - r.top) / r.height) * ex.empire_height());
  return [x, y];
}

// --- Sound: square-wave beeps in the spirit of the PC speaker.
let audio = null;
function beep(freq, ms, when = 0, vol = 0.05) {
  if (!audio) audio = new AudioContext();
  const t = audio.currentTime + when;
  const osc = audio.createOscillator();
  const gain = audio.createGain();
  osc.type = 'square';
  osc.frequency.setValueAtTime(freq, t);
  gain.gain.setValueAtTime(vol, t);
  gain.gain.setValueAtTime(0, t + ms / 1000);
  osc.connect(gain).connect(audio.destination);
  osc.start(t);
  osc.stop(t + ms / 1000 + 0.02);
}

const SOUNDS = {
  move: () => beep(1800, 4, 0, 0.02),
  error: () => beep(180, 140),
  win: () => [523, 659, 784].forEach((f, i) => beep(f, 70, i * 0.08)),
  lose: () => [392, 330, 262].forEach((f, i) => beep(f, 90, i * 0.1)),
  capture: () => [523, 659, 784, 1047].forEach((f, i) => beep(f, 80, i * 0.09)),
  gameover: () => [392, 392, 392, 311, 466, 392].forEach((f, i) => beep(f, 160, i * 0.2)),
};

function handleRequests() {
  const ptr = ex.empire_requests();
  const len = ex.empire_requests_len();
  if (len <= 2) return;
  const reqs = JSON.parse(dec.decode(new Uint8Array(ex.memory.buffer, ptr, len)));
  for (const r of reqs) {
    switch (r.type) {
      case 'store':
        store(r.key, r.value);
        break;
      case 'sound':
        SOUNDS[r.name]?.();
        break;
      case 'download': {
        const blob = new Blob([r.text], { type: r.filename.endsWith('.json') ? 'application/json' : 'text/plain' });
        const a = document.createElement('a');
        a.href = URL.createObjectURL(blob);
        a.download = r.filename;
        a.click();
        setTimeout(() => URL.revokeObjectURL(a.href), 1000);
        break;
      }
      case 'upload':
        fileInput.accept = r.accept;
        fileInput.value = '';
        fileInput.click();
        break;
      case 'quit':
        setStatus('THANK YOU FOR PLAYING EMPIRE. RELOAD TO PLAY AGAIN.');
        running = false;
        ctx.fillStyle = '#000';
        ctx.fillRect(0, 0, canvas.width, canvas.height);
        break;
    }
  }
}

// Redraw timings, readable from the console as window.empireStats.
const stats = { frames: 0, lastMs: 0, avgMs: 0, maxMs: 0, size: '' };
window.empireStats = stats;
function recordFrame(ms, w, h) {
  stats.frames++;
  stats.lastMs = Math.round(ms * 10) / 10;
  stats.avgMs = Math.round((stats.avgMs * (stats.frames - 1) + ms) / stats.frames * 10) / 10;
  stats.maxMs = Math.max(stats.maxMs, stats.lastMs);
  stats.size = `${w}x${h}`;
}

let running = true;
function frame(now) {
  if (!running) return;
  try {
    // Only redraw when something on screen changed.
    if (ex.empire_tick(now)) {
      const t0 = performance.now();
      const ptr = ex.empire_render();
      const w = ex.empire_pixel_width();
      const h = ex.empire_pixel_height();
      if (w !== canvas.width || h !== canvas.height) fitCanvas();
      fitCanvas();
      const pixels = new Uint8ClampedArray(ex.memory.buffer, ptr, w * h * 4);
      ctx.putImageData(new ImageData(pixels, w, h), 0, 0);
      recordFrame(performance.now() - t0, w, h);
    }
    handleRequests();
  } catch (e) {
    running = false;
    console.error(e);
    setStatus('THE GAME HAS STOPPED WITH AN ERROR (SEE THE CONSOLE). YOUR LAST TURN WAS AUTOSAVED.');
    return;
  }
  requestAnimationFrame(frame);
}

function attachInput() {
  window.addEventListener('keydown', (e) => {
    const mods = (e.ctrlKey ? 1 : 0) | (e.altKey ? 2 : 0) | (e.shiftKey ? 4 : 0) | (e.metaKey ? 8 : 0);
    const used = withStr2(e.key, e.code, (a, b, c, d) => ex.empire_key(a, b, c, d, mods));
    if (used) e.preventDefault();
  });
  canvas.addEventListener('mousedown', (e) => {
    canvas.focus();
    const [x, y] = toFrame(e);
    ex.empire_mouse_down(x, y, e.button);
    e.preventDefault();
  });
  canvas.addEventListener('mousemove', (e) => {
    const [x, y] = toFrame(e);
    ex.empire_mouse_move(x, y);
  });
  canvas.addEventListener('contextmenu', (e) => e.preventDefault());
  canvas.addEventListener(
    'wheel',
    (e) => {
      ex.empire_wheel(Math.sign(e.deltaX), Math.sign(e.deltaY));
      e.preventDefault();
    },
    { passive: false },
  );
  fileInput.addEventListener('change', async () => {
    const f = fileInput.files?.[0];
    if (!f) return;
    const text = await f.text();
    withStr2(f.name, text, (a, b, c, d) => ex.empire_file_loaded(a, b, c, d));
  });
  window.addEventListener('resize', resize);
}

async function main() {
  try {
    await loadModule();
  } catch (e) {
    setStatus(`COULD NOT LOAD THE GAME: ${e.message}`);
    throw e;
  }
  await loadStorage();
  const n = await loadOriginalAssets();
  console.info(n ? `loaded ${n} original asset files` : 'using built-in art');
  attachInput();
  resize();
  setStatus('');
  canvas.focus();
  requestAnimationFrame(frame);
}

main();
