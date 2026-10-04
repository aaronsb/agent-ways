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
import { pathToFileURL } from 'node:url';

const WIDTH = 1920;
const HEIGHT = 1080;

function parseArgs(argv) {
  const args = { page: null, out: null, fps: 30, stills: null, chromium: process.env.CHROMIUM || 'chromium' };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--out') args.out = argv[++i];
    else if (a === '--fps') args.fps = Number(argv[++i]);
    else if (a === '--stills') args.stills = argv[++i].split(',').map(Number);
    else if (a === '--chromium') args.chromium = argv[++i];
    else if (!args.page) args.page = a;
    else throw new Error(`unexpected argument: ${a}`);
  }
  if (!args.page) throw new Error('usage: record.mjs <page.html> [--out FILE|DIR] [--fps N] [--stills t1,t2,...]');
  const name = path.basename(path.dirname(path.resolve(args.page)));
  args.out ??= args.stills ? path.join('build', 'explainers', `${name}-stills`) : path.join('build', 'explainers', `${name}.mp4`);
  return args;
}

// Start Chromium and return the browser's DevTools WebSocket URL.
async function launch(chromium) {
  const profile = mkdtempSync(path.join(tmpdir(), 'explainer-chromium-'));
  const proc = spawn(chromium, [
    '--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`,
    '--hide-scrollbars', '--force-device-scale-factor=1', '--font-render-hinting=none',
    '--no-first-run', '--no-default-browser-check', '--allow-file-access-from-files', 'about:blank',
  ], { stdio: ['ignore', 'ignore', 'pipe'] });
  let buffered = '';
  const url = await new Promise((resolve, reject) => {
    proc.stderr.on('data', chunk => {
      buffered += chunk;
      const m = buffered.match(/DevTools listening on (ws:\/\/\S+)/);
      if (m) resolve(m[1]);
    });
    proc.on('exit', code => reject(new Error(`chromium exited (${code}) before DevTools came up:\n${buffered}`)));
  });
  return { proc, url, profile };
}

// A minimal DevTools protocol client: one socket, calls matched by id.
async function connect(url) {
  const ws = new WebSocket(url);
  await once(ws, 'open');
  let nextId = 1;
  const pending = new Map();
  const waiters = [];
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
    const id = nextId++;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
  });
  const event = method => new Promise(resolve => waiters.push({ method, resolve }));
  return { ws, send, event };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const pageUrl = pathToFileURL(path.resolve(args.page)).href + '?record=1';
  const { proc, url, profile } = await launch(args.chromium);
  const cdp = await connect(url);
  try {
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

    // Seek, then wait one animation frame so the screenshot sees the new state.
    const frameAt = async (t, format) => {
      await evaluate(`(window.__seek(${t}), new Promise(r => requestAnimationFrame(() => r())))`);
      const { data } = await call('Page.captureScreenshot', { format, ...(format === 'jpeg' ? { quality: 92 } : {}) });
      return Buffer.from(data, 'base64');
    };

    if (args.stills) {
      mkdirSync(args.out, { recursive: true });
      for (const t of args.stills) {
        const file = path.join(args.out, `t${String(t).padStart(6, '0')}.png`);
        writeFileSync(file, await frameAt(t, 'png'));
        console.log('wrote', file);
      }
      return;
    }

    mkdirSync(path.dirname(args.out), { recursive: true });
    const ff = spawn('ffmpeg', ['-y', '-loglevel', 'error', '-f', 'image2pipe', '-framerate', String(args.fps), '-vcodec', 'mjpeg', '-i', '-',
      '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-crf', '18', '-preset', 'slow', '-movflags', '+faststart', args.out],
      { stdio: ['pipe', 'inherit', 'inherit'] });
    const total = Math.round(duration * args.fps);
    for (let i = 0; i < total; i++) {
      const buf = await frameAt(i / args.fps, 'jpeg');
      if (!ff.stdin.write(buf)) await once(ff.stdin, 'drain');
      if (i % (args.fps * 10) === 0) console.log(`frame ${i}/${total}`);
    }
    ff.stdin.end();
    const [code] = await once(ff, 'close');
    if (code !== 0) throw new Error(`ffmpeg exited ${code}`);
    console.log(`wrote ${args.out} (${duration}s at ${args.fps} fps)`);
  } finally {
    cdp.ws.close();
    proc.kill('SIGTERM');
    await once(proc, 'exit').catch(() => {});
    rmSync(profile, { recursive: true, force: true });
  }
}

main().catch(err => {
  console.error(`record: ${err.message}`);
  process.exit(1);
});
