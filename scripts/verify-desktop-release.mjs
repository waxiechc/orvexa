// Verify an extracted release independently of the source web/dist directory.
// Run: node scripts/verify-desktop-release.mjs <release-directory>
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { spawn, spawnSync } from 'node:child_process';
import { readFile, writeFile, mkdtemp, mkdir, stat } from 'node:fs/promises';
import { resolve, join, extname, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const require = createRequire(join(root, 'web/package.json'));
const { chromium } = require('@playwright/test');
assert.equal(process.platform, 'win32', 'This release check requires Windows');
assert.ok(process.argv[2], 'Pass the packaged release directory');
const release = resolve(process.argv[2]);
await mkdir(join(root, 'demo-data'), { recursive: true });
const proof = await mkdtemp(join(root, 'demo-data/desktop-release-'));
const extracted = join(proof, 'extracted');
const zip = join(release, 'ORVEXA-Desktop-Windows-x64.zip');
const extraction = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
  'Expand-Archive -LiteralPath $env:ORVEXA_VERIFY_ZIP -DestinationPath $env:ORVEXA_VERIFY_OUT'],
{ env: { ...process.env, ORVEXA_VERIFY_ZIP: zip, ORVEXA_VERIFY_OUT: extracted }, encoding: 'utf8', windowsHide: true });
assert.equal(extraction.status, 0, extraction.stderr);
const sums = await readFile(join(release, 'SHA256SUMS.txt'), 'utf8');
for (const line of sums.trim().split(/\r?\n/)) {
  const [hash, name] = line.split(/\s{2}/);
  assert.match(name, /^(ORVEXA\.exe|ORVEXA-Desktop-Windows-x64\.zip)$/);
  assert.equal(createHash('sha256').update(await readFile(join(release, name))).digest('hex'), hash);
}
assert.deepEqual(await readFile(join(extracted, 'ORVEXA.exe')), await readFile(join(release, 'ORVEXA.exe')));
const child = spawn(join(extracted, 'ORVEXA.exe'), ['--headless', '--listen', '127.0.0.1:0', '--data-dir', join(proof, 'profile')],
  { cwd: extracted, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
let browser, site;
try {
  const origin = await new Promise((ok, fail) => {
    let output = '';
    const timer = setTimeout(() => fail(new Error('Packaged app did not start')), 20000);
    child.stdout.on('data', chunk => {
      output += chunk.toString();
      const match = output.match(/ORVEXA_DESKTOP_ORIGIN=(http:\/\/127\.0\.0\.1:\d+)/);
      if (match) { clearTimeout(timer); ok(match[1]); }
    });
    child.once('exit', code => { clearTimeout(timer); fail(new Error(`App exited: ${code}`)); });
    child.once('error', fail);
  });
  const status = await (await fetch(`${origin}/v1/desktop/status`)).json();
  assert.equal(status.validator_active, false);
  assert.equal(status.rewards_supported, false);
  assert.equal((await fetch(`${origin}/v1/status`)).status, 503);
  const html = await (await fetch(origin)).text(); assert.ok(html.includes('ORVEXA'));
  const wasm = await fetch(`${origin}/wasm/orvexa_web_wallet_bg.wasm`);
  assert.equal(wasm.status, 200); assert.ok((await wasm.arrayBuffer()).byteLength > 100000);
  const notices = await fetch(`${origin}/open-source-notices.txt`);
  assert.match(notices.headers.get('content-type'), /^text\/plain/);
  assert.ok((await notices.text()).includes('Permission is hereby granted'));

  const docs = join(root, 'docs');
  site = createServer(async (req, res) => {
    try {
      const pathname = decodeURIComponent(new URL(req.url, 'http://localhost').pathname);
      const file = resolve(docs, `.${pathname === '/' ? '/index.html' : pathname}`);
      if (!file.startsWith(docs + sep)) { res.writeHead(403).end(); return; }
      const data = await readFile(file);
      const types = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.png': 'image/png', '.svg': 'image/svg+xml', '.json': 'application/json' };
      res.writeHead(200, { 'content-type': types[extname(file)] || 'application/octet-stream' }); res.end(data);
    } catch { res.writeHead(404).end(); }
  });
  await new Promise(ok => site.listen(0, '127.0.0.1', ok));
  const siteOrigin = `http://127.0.0.1:${site.address().port}`;
  browser = await chromium.launch(); const page = await browser.newPage();
  const errors = []; page.on('pageerror', e => errors.push(e.message));
  const responses = []; page.on('response', r => { if (r.status() >= 400) responses.push(`${r.status()} ${r.url()}`); });
  await page.goto(`${origin}/#/node`);
  await page.getByRole('heading', { name: 'Connect your node' }).waitFor();
  // A stopped full node intentionally returns 503 for ledger queries.
  responses.length = 0;
  await page.goto(siteOrigin);
  await page.getByRole('heading', { name: /Your wallet.*Your node.*Your network/ }).waitFor();
  assert.ok((await page.locator('#exe-size').innerText()).includes('MB'));
  for (const [name, width, height] of [['site-desktop', 1440, 1000], ['site-mobile', 390, 844]]) {
    await page.setViewportSize({ width, height });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'Horizontal overflow');
    await page.screenshot({ path: join(proof, `${name}.png`), fullPage: true });
  }
  await page.keyboard.press('Tab');
  assert.equal(await page.locator(':focus').innerText(), 'Skip to content');
  const downloads = await page.locator('[data-download]').evaluateAll(links => links.map(link => link.href));
  assert.equal(downloads.length, 3);
  assert.ok(downloads.every(link => link.includes('/releases/download/v0.1.0-desktop-preview.1/')));
  assert.deepEqual(errors, []); assert.deepEqual(responses, []);
  const result = { result: 'PASS', extracted_exe: true, embedded_wasm: true, embedded_licenses: true,
    checksum_match: true, site_desktop_mobile: true, no_horizontal_overflow: true, screenshots: proof,
    exe_bytes: (await stat(join(release, 'ORVEXA.exe'))).size };
  await writeFile(join(proof, 'result.json'), JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result));
} finally {
  await browser?.close();
  if (site) await new Promise(ok => site.close(ok));
  if (child.exitCode === null) await new Promise(ok => { child.once('exit', ok); child.kill(); });
}
