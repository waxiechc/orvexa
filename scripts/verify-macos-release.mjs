// Archive/architecture/controller verification. Native window QA is separate.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, mkdirSync, mkdtempSync, existsSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
assert.equal(process.platform, 'darwin');
assert.ok(process.argv[2], 'Pass a packaged Mac release directory');
const root = resolve(import.meta.dirname, '..'), release = resolve(process.argv[2]);
mkdirSync(resolve(root, 'demo-data'), { recursive: true });
const proof = mkdtempSync(resolve(root, 'demo-data/macos-release-'));
const extracted = resolve(proof, 'extracted');
function run(binary, args) {
  const r = spawnSync(binary, args, { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr); return r.stdout.trim();
}
const sums = readFileSync(resolve(release, 'SHA256SUMS-macOS.txt'), 'utf8');
for (const line of sums.trim().split(/\r?\n/)) {
  const [hash, name] = line.split(/\s{2}/);
  assert.match(name, /^ORVEXA-macOS-(universal|arm64|x64)\.zip$/);
  const zip = resolve(release, name);
  assert.equal(createHash('sha256').update(readFileSync(zip)).digest('hex'), hash);
  run('ditto', ['-x', '-k', zip, extracted]);
}
const app = resolve(extracted, 'ORVEXA.app'), binary = resolve(app, 'Contents/MacOS/ORVEXA');
for (const file of ['MACOS.md', 'DESKTOP_APP.md', 'LICENSE', 'SHA256SUMS-app.txt', 'ORVEXA.app/Contents/Resources/third-party-licenses/index.json']) assert.ok(existsSync(resolve(extracted, file)), file);
const [binaryHash, binaryName] = readFileSync(resolve(extracted, 'SHA256SUMS-app.txt'), 'utf8').trim().split(/\s{2}/);
assert.equal(binaryName, 'ORVEXA.app/Contents/MacOS/ORVEXA');
assert.equal(createHash('sha256').update(readFileSync(binary)).digest('hex'), binaryHash);
const architectures = run('lipo', ['-archs', binary]);
if (sums.includes('universal')) { assert.match(architectures, /arm64/); assert.match(architectures, /x86_64/); }
run('codesign', ['--verify', '--strict', app]);
const child = spawn(binary, ['--headless', '--data-dir', resolve(proof, 'profile'), '--listen', '127.0.0.1:0'], { cwd: extracted, stdio: ['ignore', 'pipe', 'pipe'] });
try {
  const origin = await new Promise((ok, fail) => {
    let output = ''; const timer = setTimeout(() => fail(Error('Controller timeout')), 20000);
    child.stdout.on('data', chunk => { output += chunk; const match = output.match(/ORVEXA_DESKTOP_ORIGIN=(http:\/\/127\.0\.0\.1:\d+)/); if (match) { clearTimeout(timer); ok(match[1]); } });
    child.once('error', fail); child.once('exit', code => { clearTimeout(timer); fail(Error(`App exited ${code}`)); });
  });
  const status = await (await fetch(origin + '/v1/desktop/status')).json();
  assert.equal(status.phase, 'stopped'); assert.equal(status.validator_active, false); assert.equal(status.rewards_supported, false);
  assert.equal((await fetch(origin + '/v1/status')).status, 503);
  assert.ok((await (await fetch(origin + '/wasm/orvexa_web_wallet_bg.wasm')).arrayBuffer()).byteLength > 100000);
  assert.match(await (await fetch(origin + '/open-source-notices.txt')).text(), /Permission is hereby granted/);
  const result = { result: 'PASS', extracted_app: app, architectures, embedded_wasm_and_licenses: true, sha256_verified: true, native_window_test: 'separate manual QA required', runtime_architecture: process.arch };
  writeFileSync(resolve(proof, 'result.json'), JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result));
} finally { if (child.exitCode === null) await new Promise(ok => { child.once('exit', ok); child.kill('SIGINT'); }); }
