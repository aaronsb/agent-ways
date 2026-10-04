#!/usr/bin/env node
// Render a time-driven explainer page to MP4, or to PNG stills.
//
// An explainer is an HTML page that keeps its whole state a function of one
// clock and exposes two globals: `window.__duration` (seconds) and
// `window.__seek(t)` (draw the frame at time t). With `?record=1` in its URL
// it hides its own controls and fills a 1920x1080 viewport. See
// docs/explainers/authoring.md.
//
// This script drives headless Chromium over the DevTools protocol with
// Node's built-in WebSocket, seeks the page frame by frame, and pipes the
// frames into ffmpeg. It needs node (22+), chromium and ffmpeg; no npm
// packages.
//
//   node scripts/explainer/record.mjs docs/explainers/animations/a-turn-with-ways/index.html
//   node scripts/explainer/record.mjs <page> --out build/explainers/x.mp4 --fps 30
//   node scripts/explainer/record.mjs <page> --stills 5,20,40 --out /tmp/stills

import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const WIDTH = 1920;
const HEIGHT = 1080;
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const USAGE = 'usage: record.mjs <page.html> [--out FILE|DIR] [--fps N] [--stills t1,t2,...]';

function parseArgs(argv) {
  const args = { page: null, out: null, fps: 30, stills: null, chromium: process.env.CHROMIUM || 'chromium' };
  const value = (flag, v) => {
    if (!v) throw new Error(`${flag} needs a value\n${USAGE}`);
    return v;
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--out') args.out = value(a, argv[++i]);
    else if (a === '--fps') args.fps = Number(value(a, argv[++i]));
    else if (a === '--stills') args.stills = value(a, argv[++i]).split(',').map(Number);
    else if (a === '--chromium') args.chromium = value(a, argv[++i]);
    else if (!args.page) args.page = a;
    else throw new Error(`unexpected argument: ${a}`);
  }
  if (!args.page) throw new Error(USAGE);
  if (!Number.isInteger(args.fps) || args.fps <= 0) throw new Error(`--fps must be a positive integer, got ${args.fps}`);
  if (args.stills?.some(t => !Number.isFinite(t) || t < 0)) throw new Error('--stills takes comma-separated times in seconds, e.g. 5,20.5,40');
  const name = path.basename(path.dirname(path.resolve(args.page)));
  args.out ??= path.join(REPO, 'build', 'explainers', args.stills ? `${name}-stills` : `${name}.mp4`);
  return args;
}

// Start Chromium. `ready` resolves to the browser's DevTools WebSocket URL;
// `exited` resolves when the process is gone, whether it ran or failed to spawn.
function launch(chromium, profile) {
  const proc = spawn(chromium, [
    '--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`,
    '--hide-scrollbars', '--force-device-scale-factor=1', '--font-render-hinting=none',
    '--no-first-run', '--no-default-browser-check', '--allow-file-access-from-files', 'about:blank',
  ], { stdio: ['ignore', 'ignore', 'pipe'] });
  const exited = new Promise(resolve => { proc.on('exit', resolve); proc.on('error', resolve); });
  let buffered = '';
  const ready = new Promise((resolve, reject) => {
    proc.stderr.on('data', chunk => {
      buffered += chunk;
      const m = buffered.match(/DevTools listening on (ws:\/\/\S+)/);
      if (m) resolve(m[1]);
    });
    proc.on('error', err => reject(new Error(`cannot start ${chromium} (${err.code}); set CHROMIUM or pass --chromium`)));
    proc.on('exit', code => reject(new Error(`chromium exited (${code}) before DevTools came up:\n${buffered}`)));
  });
  return { proc, ready, exited };
}

// A minimal DevTools protocol client: one socket, calls matched by id.
async function connect(url) {
  const ws = new WebSocket(url);
  await new Promise((resolve, reject) => {
    ws.addEventListener('open', resolve, { once: true });
    ws.addEventListener('error', () => reject(new Error(`cannot connect to DevTools at ${url}`)), { once: true });
  });
  let nextId = 1;
  const pending = new Map();
  const waiters = [];
  // A closed socket means Chromium is gone: fail every outstanding call and wait.
  ws.addEventListener('close', () => {
    const err = new Error('DevTools connection closed (did chromium crash?)');
    for (const p of pending.values()) p.reject(err);
    pending.clear();
    for (const w of waiters.splice(0)) w.reject(err);
  });
  ws.addEventListener('message', ev => {
    const msg = JSON.parse(ev.data);
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      msg.error ? reject(new Error(`${msg.error.message} (${msg.error.code})`)) : resolve(msg.result);
    } else if (msg.method) {
      for (const w of waiters.splice(0)) (w.method === msg.method ? w.resolve(msg.params) : waiters.push(w));
    }
  });
  const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
    if (ws.readyState !== WebSocket.OPEN) return reject(new Error('DevTools connection closed (did chromium crash?)'));
    const id = nextId++;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
  });
  const event = method => new Promise((resolve, reject) => waiters.push({ method, resolve, reject }));
  return { ws, send, event };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const pageUrl = pathToFileURL(path.resolve(args.page)).href + '?record=1';
  const profile = mkdtempSync(path.join(tmpdir(), 'explainer-chromium-'));
  const browser = launch(args.chromium, profile);
  let cdp = null;
  let ff = null;
  let complete = false;
  try {
    cdp = await connect(await browser.ready);
    const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
    const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
    const call = (method, params) => cdp.send(method, params, sessionId);
    const evaluate = async expression => {
      const r = await call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
      if (r.exceptionDetails) throw new Error(`page error: ${r.exceptionDetails.exception?.description ?? r.exceptionDetails.text}`);
      return r.result.value;
    };

    await call('Page.enable');
    await call('Emulation.setDeviceMetricsOverride', { width: WIDTH, height: HEIGHT, deviceScaleFactor: 1, mobile: false });
    const loaded = cdp.event('Page.loadEventFired');
    await call('Page.navigate', { url: pageUrl });
    await loaded;
    const duration = await evaluate('document.fonts.ready.then(() => window.__duration)');
    if (typeof duration !== 'number' || !(duration > 0)) throw new Error('page does not expose window.__duration; is it an explainer?');
    // A web font that failed to load falls back silently, so the render would
    // not match the last one. Refuse rather than record the fallback.
    const missing = await evaluate(`Promise.all((window.__fonts || []).map(f =>
      document.fonts.load('16px "' + f + '"').then(faces => (faces.length ? null : f), () => f))).then(r => r.filter(Boolean))`);
    if (missing.length) throw new Error(`fonts did not load (offline?): ${[...new Set(missing)].join(', ')}`);

    // Seek, then wait one animation frame so the screenshot sees the new state.
    const frameAt = async (t, format) => {
      await evaluate(`(window.__seek(${t}), new Promise(r => requestAnimationFrame(() => r())))`);
      const { data } = await call('Page.captureScreenshot', { format, ...(format === 'jpeg' ? { quality: 92 } : {}) });
      return Buffer.from(data, 'base64');
    };

    if (args.stills) {
      mkdirSync(args.out, { recursive: true });
      for (const t of args.stills) {
        const file = path.join(args.out, `t${t.toFixed(2).padStart(9, '0')}.png`);  // sorts in time order
        writeFileSync(file, await frameAt(t, 'png'));
        console.log('wrote', file);
      }
      complete = true;
      return;
    }

    mkdirSync(path.dirname(args.out), { recursive: true });
    ff = spawn('ffmpeg', ['-y', '-loglevel', 'error', '-f', 'image2pipe', '-framerate', String(args.fps), '-vcodec', 'mjpeg', '-i', '-',
      '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-crf', '18', '-preset', 'slow', '-movflags', '+faststart', args.out],
      { stdio: ['pipe', 'inherit', 'inherit'] });
    ff.stdin.on('error', () => {});  // a dead ffmpeg surfaces through ffDone
    // Settles when ffmpeg is gone for any reason; an early exit rejects so a
    // pending write cannot wait on 'drain' forever.
    const ffDone = new Promise((resolve, reject) => {
      ff.on('error', err => reject(new Error(`cannot start ffmpeg (${err.code})`)));
      ff.on('close', code => (code === 0 ? resolve() : reject(new Error(`ffmpeg exited ${code}`))));
    });
    ffDone.catch(() => {});
    let finishing = false;
    const ffEarly = ffDone.then(() => { if (!finishing) throw new Error('ffmpeg exited before the last frame'); });
    ffEarly.catch(() => {});
    const total = Math.round(duration * args.fps);
    for (let i = 0; i < total; i++) {
      const buf = await Promise.race([frameAt(i / args.fps, 'jpeg'), ffEarly]);
      if (!ff.stdin.write(buf)) await Promise.race([once(ff.stdin, 'drain'), ffEarly]);
      if (i % (args.fps * 10) === 0) console.log(`frame ${i}/${total}`);
    }
    finishing = true;
    ff.stdin.end();
    await ffDone;
    complete = true;
    console.log(`wrote ${args.out} (${duration}s at ${args.fps} fps)`);
  } finally {
    if (ff && ff.exitCode === null && ff.signalCode === null) ff.kill('SIGKILL');
    if (ff && !complete) rmSync(args.out, { force: true });  // no truncated MP4
    // Browser.close shuts Chromium down in order, so its child processes stop
    // writing to the profile before the main process exits. SIGTERM is the fallback.
    const running = () => browser.proc.exitCode === null && browser.proc.signalCode === null;
    if (cdp && running()) await Promise.race([cdp.send('Browser.close').catch(() => {}), new Promise(r => setTimeout(r, 2000))]);
    cdp?.ws.close();
    const stopped = await Promise.race([browser.exited.then(() => true), new Promise(r => setTimeout(r, 5000, false))]);
    if (!stopped && running()) browser.proc.kill('SIGTERM');
    await browser.exited;
    rmSync(profile, { recursive: true, force: true });
  }
}

main().catch(err => {
  console.error(`record: ${err.message}`);
  process.exit(1);
});
