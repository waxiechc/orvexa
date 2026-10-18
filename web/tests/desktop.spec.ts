import { test, expect, chromium, type Page } from '@playwright/test';
import { spawn, spawnSync, type ChildProcess } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { randomUUID } from 'node:crypto';

const root = resolve(import.meta.dirname, '../..');
const bin = resolve(root, process.env.ORVEXA_BIN_DIR || 'target/debug');
const extension = process.platform === 'win32' ? '.exe' : '';
const cli = resolve(bin, `orvexa-cli${extension}`); const node = resolve(bin, `orvexa-node${extension}`); const app = resolve(bin, `orvexa-desktop${extension}`);
const origin = 'http://127.0.0.1:59242'; const authorityOrigin = 'http://127.0.0.1:59243';
const secret = `Disposable-${randomUUID()}`;
let directory: string, appChild: ChildProcess, authorityChild: ChildProcess, recipient: string;
function command(binary: string, args: string[], password?: string) {
  const r = spawnSync(binary, args, { encoding: 'utf8', input: password, windowsHide: true });
  if (r.status !== 0) throw new Error(`Native operation failed: ${r.stderr}`); return r.stdout.trim();
}
async function stop(child?: ChildProcess) { if (child && child.exitCode === null) await new Promise<void>(resolve => { child.once('exit', () => resolve()); child.kill(); }); }
async function startApp() {
  const args = ['--headless', '--listen', '127.0.0.1:59242', '--data-dir', resolve(directory, 'app')];
  if (!process.env.ORVEXA_TEST_EMBEDDED) args.push('--web-dir', resolve(root, 'web/dist'));
  appChild = spawn(app, args, { stdio: 'ignore', windowsHide: true });
  await expect.poll(async () => { try { return (await fetch(`${origin}/v1/desktop/status`)).ok; } catch { return false; } }).toBe(true);
}
async function capture(page: Page, name: string) {
  await page.evaluate(async () => {
    (document.activeElement as HTMLElement)?.blur();
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await Promise.all(document.getAnimations().filter(a => Number.isFinite(a.effect?.getComputedTiming().endTime)).map(a => a.finished.catch(() => undefined)));
  });
  await expect.poll(() => page.locator('.skip-link').evaluate(e => e.getBoundingClientRect().bottom)).toBeLessThanOrEqual(0);
  await page.screenshot({ path: resolve(directory, name), fullPage: true });
}
test.beforeAll(async () => {
  mkdirSync(resolve(root, 'demo-data'), { recursive: true }); directory = mkdtempSync(resolve(root, 'demo-data/desktop-e2e-'));
  const authority = command(cli, ['wallet-new', '--wallet', resolve(directory, 'authority.wallet.json'), '--password-stdin'], secret);
  recipient = command(cli, ['wallet-new', '--wallet', resolve(directory, 'owner.wallet.json'), '--password-stdin'], secret);
  const result = JSON.parse(command(cli, ['genesis', '--authority', authority, '--recipient', recipient, '--out', resolve(directory, 'genesis.json')]));
  const peer = command(node, ['peer-id', '--identity', resolve(directory, 'authority.peerkey')]);
  const seed = `/ip4/127.0.0.1/tcp/59244/p2p/${peer}`;
  writeFileSync(resolve(directory, 'network.json'), JSON.stringify({ profile: 'orvexa-p2p-v1', chain_id: result.chain_id, seeds: [seed] }));
  command(node, ['init', '--db', resolve(directory, 'authority.sqlite'), '--genesis', resolve(directory, 'genesis.json')]);
  authorityChild = spawn(node, ['run', '--db', resolve(directory, 'authority.sqlite'), '--listen', '127.0.0.1:59243', '--p2p-listen', '/ip4/127.0.0.1/tcp/59244', '--p2p-identity', resolve(directory, 'authority.peerkey'), '--expect-chain', result.chain_id, '--allow-private-peers', '--authority-wallet', resolve(directory, 'authority.wallet.json'), '--password-stdin', '--block-every', '1', '--sync-every', '1'], { stdio: ['pipe', 'ignore', 'ignore'], windowsHide: true });
  authorityChild.stdin!.end(secret);
  await expect.poll(async () => { try { return (await fetch(`${authorityOrigin}/v1/status`)).ok; } catch { return false; } }).toBe(true);
  await startApp();
});
test.afterAll(async () => { await stop(appChild); await stop(authorityChild); });

test('real desktop controller joins one chain, transfers, stops and resumes', async ({ page }) => {
  const errors: string[] = []; page.on('pageerror', e => errors.push(e.message));
  await page.setViewportSize({ width: 1440, height: 1000 }); await page.goto(`${origin}/#/node`);
  await expect(page.getByRole('heading', { name: 'Connect your node' })).toBeVisible();
  await capture(page, 'node-setup-desktop.png');
  await page.setViewportSize({ width: 390, height: 844 }); await capture(page, 'node-setup-mobile.png'); await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByLabel('Genesis file').setInputFiles(resolve(directory, 'genesis.json'));
  await page.getByLabel('Network file').setInputFiles(resolve(directory, 'network.json'));
  await page.getByLabel('Peer port').fill('59245');
  await page.getByLabel('I checked this chain ID').check(); await page.getByLabel('Resume this node').check(); await page.getByLabel('Allow private peer').check();
  await page.getByRole('button', { name: 'Verify & save network' }).click(); await expect(page.getByRole('heading', { name: 'Node stopped' })).toBeVisible();
  await page.getByRole('button', { name: 'Start node', exact: true }).click(); await expect(page.getByRole('heading', { name: 'Node running' })).toBeVisible();
  await expect.poll(async () => (await (await fetch(`${origin}/v1/desktop/status`)).json()).runtime?.peers.length).toBeGreaterThan(0);
  const runtime = await (await fetch(`${origin}/v1/desktop/status`)).json(); expect(runtime.validator_active).toBe(false); expect(runtime.rewards_supported).toBe(false);
  await page.goto(`${origin}/#/wallets`); await page.getByRole('button', { name: 'Restore wallet', exact: true }).click();
  const dialog = page.getByRole('dialog'); await dialog.getByLabel('Wallet name').fill('Owner'); await dialog.getByLabel('Encrypted wallet file').setInputFiles(resolve(directory, 'owner.wallet.json')); await dialog.getByLabel('Backup password').fill(secret); await dialog.getByRole('button', { name: 'Restore wallet', exact: true }).click();
  await expect(dialog.getByText('Your wallet is ready')).toBeVisible(); const download = page.waitForEvent('download'); await dialog.getByRole('button', { name: 'Download encrypted backup' }).click(); await download; await dialog.getByRole('button', { name: 'Continue to wallet' }).click();
  await page.goto(`${origin}/#/send`); await page.getByLabel('Recipient address').fill(recipient); await page.getByLabel('Amount', { exact: false }).fill('1'); await page.getByLabel('Network fee', { exact: false }).fill('1'); await page.getByRole('button', { name: 'Review transfer' }).click(); await page.getByLabel('Wallet password', { exact: true }).fill(secret); await page.getByRole('button', { name: 'Sign & send' }).click(); await expect(page.getByText('Transfer submitted', { exact: true })).toBeVisible();
  await expect.poll(async () => (await (await fetch(`${origin}/v1/status`)).json()).height).toBe(1);
  const chain = await (await fetch(`${origin}/v1/status`)).json();
  await page.goto(`${origin}/#/node`); await expect(page.getByText('#1', { exact: true }).first()).toBeVisible(); await capture(page, 'node-running-desktop.png');
  await page.setViewportSize({ width: 390, height: 844 }); await capture(page, 'node-running-mobile.png'); await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByRole('button', { name: 'Stop node', exact: true }).click(); await expect(page.getByRole('heading', { name: 'Node stopped' })).toBeVisible(); expect((await fetch(`${origin}/v1/status`)).status).toBe(503);
  await page.route('**/v1/desktop/status', route => route.abort());
  await expect(page.getByRole('heading', { name: 'Node status unavailable' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Start node', exact: true })).toBeDisabled();
  await page.unroute('**/v1/desktop/status'); await expect(page.getByRole('heading', { name: 'Node stopped' })).toBeVisible();
  await stop(appChild); await startApp(); await page.reload(); await expect(page.getByRole('heading', { name: 'Node running' })).toBeVisible();
  await expect.poll(async () => (await (await fetch(`${origin}/v1/status`)).json()).tip).toBe(chain.tip);
  const blocks = await (await fetch(`${origin}/v1/explorer/blocks`)).json(); await page.goto(`${origin}/#/explorer/tx/${blocks[0].transaction_ids[0]}`); await expect(page.getByRole('heading', { name: 'Transaction', exact: true })).toBeVisible(); await expect(page.getByText('Confirmed', { exact: true })).toBeVisible();
  expect(errors).toEqual([]); writeFileSync(resolve(directory, 'result.json'), JSON.stringify({ result: 'PASS', control_and_native_ledger: true, profile_resumed: true, height: 1, tip: chain.tip, consensus: 'single authority, not BFT', privacy: 'transparent v1' }, null, 2));
});

test('native Windows window renders embedded app and executes local Wasm', async () => {
  test.skip(process.platform !== 'win32' || !process.env.ORVEXA_NATIVE_GUI_TEST, 'Explicit Windows WebView2 test');
  const nativeOrigin = 'http://127.0.0.1:60242';
  const child = spawn(app, ['--listen', '127.0.0.1:60242', '--data-dir', resolve(directory, 'native-window'), '--webview-debug-port', '60243'], { windowsHide: true, stdio: 'ignore' });
  let browser: Awaited<ReturnType<typeof chromium.connectOverCDP>> | undefined;
  try {
    await expect.poll(async () => { try { return (await fetch('http://127.0.0.1:60243/json/version')).ok; } catch { return false; } }).toBe(true);
    browser = await chromium.connectOverCDP('http://127.0.0.1:60243');
    await expect.poll(() => browser!.contexts()[0].pages().some(p => p.url().startsWith(nativeOrigin))).toBe(true);
    const page = browser.contexts()[0].pages().find(p => p.url().startsWith(nativeOrigin))!;
    await expect(page.getByRole('heading', { name: 'Connect your node' })).toBeVisible(); await capture(page, 'native-window.png');
    await page.goto(`${nativeOrigin}/#/wallets`); await page.getByRole('button', { name: 'Create wallet', exact: true }).click();
    const dialog = page.getByRole('dialog'); await dialog.getByLabel('Wallet name').fill('Native window wallet'); await dialog.getByLabel('New password').fill(secret); await dialog.getByLabel('Repeat password').fill(secret); await dialog.getByRole('button', { name: 'Create wallet', exact: true }).click(); await expect(dialog.getByText('Your wallet is ready')).toBeVisible();
    const encrypted = await page.evaluate(() => localStorage.getItem('orvexa.encrypted-wallets.v1')); expect(encrypted).toContain('Native window wallet'); expect(encrypted).not.toContain(secret);
    writeFileSync(resolve(directory, 'native-window-result.json'), JSON.stringify({ result: 'PASS', native_webview2: true, embedded_assets: true, rust_wasm_wallet_creation: true, debug_only_explicit_flag: true }));
  } finally { await browser?.close(); await stop(child); }
});
