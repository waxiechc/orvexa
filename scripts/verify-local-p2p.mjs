// Disposable local v1 network: real Rust authority + two verifying processes.
// No credentials are printed, passed in argv, or included in result.json.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

const root = resolve(import.meta.dirname, '..');
const bin = resolve(root, process.env.ORVEXA_BIN_DIR || 'target/debug');
const ext = process.platform === 'win32' ? '.exe' : '';
const cli = resolve(bin, `orvexa-cli${ext}`), node = resolve(bin, `orvexa-node${ext}`);
mkdirSync(resolve(root, 'demo-data'), { recursive: true });
const directory = mkdtempSync(resolve(root, 'demo-data/macos-network-'));
const password = randomUUID() + randomUUID();
const children = new Map();
const origins = [61342, 61343, 61344].map(p => `http://127.0.0.1:${p}`);
function command(binary, args, secret) {
  const r = spawnSync(binary, args, { encoding: 'utf8', input: secret });
  assert.equal(r.status, 0, r.stderr || 'Rust process failed'); return r.stdout.trim();
}
const wallet = name => resolve(directory, `${name}.wallet.json`);
const authority = command(cli, ['wallet-new', '--wallet', wallet('authority'), '--password-stdin'], password);
const a = command(cli, ['wallet-new', '--wallet', wallet('a'), '--password-stdin'], password);
const b = command(cli, ['wallet-new', '--wallet', wallet('b'), '--password-stdin'], password);
const genesis = resolve(directory, 'genesis.json');
const { chain_id } = JSON.parse(command(cli, ['genesis', '--authority', authority, '--recipient', a, '--out', genesis]));
const ids = [0, 1, 2].map(i => command(node, ['peer-id', '--identity', resolve(directory, `${i}.peerkey`)]));
const seed = `/ip4/127.0.0.1/tcp/61352/p2p/${ids[0]}`;
writeFileSync(resolve(directory, 'network.json'), JSON.stringify({ profile: 'orvexa-p2p-v1', chain_id, seeds: [seed] }, null, 2));
for (let i = 0; i < 3; i++) command(node, ['init', '--db', resolve(directory, `${i}.sqlite`), '--genesis', genesis]);
async function waitFor(test, label) {
  const deadline = Date.now() + 45000;
  while (Date.now() < deadline) {
    for (const [i, child] of children) assert.equal(child.exitCode, null, `node ${i} exited`);
    try { if (await test()) return; } catch (error) { if (error.code === 'ERR_ASSERTION') throw error; }
    await delay(200);
  }
  throw Error(`Timeout: ${label}`);
}
async function json(i, path) { const r = await fetch(origins[i] + path, { signal: AbortSignal.timeout(3000) }); assert.ok(r.ok); return r.json(); }
async function start(i) {
  const args = ['run', '--db', resolve(directory, `${i}.sqlite`), '--listen', `127.0.0.1:${61342 + i}`, '--p2p-listen', `/ip4/127.0.0.1/tcp/${61352 + i}`, '--p2p-identity', resolve(directory, `${i}.peerkey`), '--expect-chain', chain_id, '--allow-private-peers', '--sync-every', '1'];
  if (i === 0) args.push('--authority-wallet', wallet('authority'), '--password-stdin', '--block-every', '1');
  else args.push('--p2p-seed', seed);
  const child = spawn(node, args, { stdio: ['pipe', 'ignore', 'ignore'] });
  children.set(i, child); child.stdin.end(i === 0 ? password : undefined);
  await waitFor(async () => (await json(i, '/v1/status')).chain_id === chain_id, `node ${i} start`);
}
async function stop(i) {
  const child = children.get(i); children.delete(i);
  if (child && child.exitCode === null) await new Promise(ok => { child.once('exit', ok); child.kill('SIGINT'); });
}
function send(i, name, to, amount, fee) {
  return JSON.parse(command(cli, ['--node', origins[i], '--expect-chain', chain_id, 'send', '--wallet', wallet(name), '--to', to, '--amount', String(amount), '--fee', String(fee), '--password-stdin'], password));
}
try {
  for (let i = 0; i < 3; i++) await start(i);
  await waitFor(async () => (await json(0, '/v1/runtime')).peers.length >= 2, 'two real Noise peers');
  const first = send(1, 'a', b, 250, 2);
  await waitFor(async () => (await Promise.all([0, 1, 2].map(i => json(i, '/v1/status')))).every(s => s.height === 1), 'A → B confirmations');
  const second = send(2, 'b', a, 40, 1);
  await waitFor(async () => (await Promise.all([0, 1, 2].map(i => json(i, '/v1/status')))).every(s => s.height === 2), 'B → A confirmations');
  const before = await Promise.all([0, 1, 2].map(i => json(i, '/v1/status')));
  for (let i = 2; i >= 0; i--) await stop(i);
  for (let i = 0; i < 3; i++) await start(i);
  const after = await Promise.all([0, 1, 2].map(i => json(i, '/v1/status')));
  assert.deepEqual(after, before);
  for (let i = 0; i < 3; i++) {
    assert.equal((await json(i, `/v1/balance/${a}`)).confirmed.balance, 999788);
    assert.equal((await json(i, `/v1/balance/${b}`)).confirmed.balance, 209);
    assert.equal(after[i].burned_fees, 3);
  }
  const blocks = await json(1, '/v1/explorer/blocks');
  const hashes = blocks.flatMap(block => block.transaction_ids);
  assert.equal(hashes.length, 2);
  // Exercise the same transaction lookup used by Explorer.
  const transactions = await Promise.all(hashes.map(hash => json(2, `/v1/transactions/${hash}`)));
  assert.ok(transactions.every(tx => tx.status === 'confirmed' && tx.confirmations >= 1));
  const result = { result: 'PASS', chain_id, a, b, authority, first, second, hashes, transactions, before, after, peer_ids: ids, balances: { a: 999788, b: 209, burned: 3 }, directory };
  writeFileSync(resolve(directory, 'result.json'), JSON.stringify(result, null, 2));
  console.log(JSON.stringify({ result: 'PASS', directory, chain_id, hashes }));
  if (process.argv.includes('--serve')) {
    // Only for the local native-window test, never included in an archive.
    writeFileSync(resolve(directory, 'test-password.secret'), password, { mode: 0o600 });
    console.log('Local test nodes remain running; SIGINT stops them. Public network files are in the reported directory.');
    await new Promise(ok => { process.once('SIGINT', ok); process.once('SIGTERM', ok); });
  }
} finally { for (const i of [...children.keys()]) await stop(i); }
