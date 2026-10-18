import { test, expect, type Page } from '@playwright/test';
import { spawn, spawnSync, type ChildProcess } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { randomUUID } from 'node:crypto';

const root = resolve(import.meta.dirname, '../..');
const extension = process.platform === 'win32' ? '.exe' : '';
const binaryDir = process.env.ORVEXA_BIN_DIR || 'target/debug';
const cli = resolve(root, binaryDir, `orvexa-cli${extension}`);
const binary = resolve(root, binaryDir, `orvexa-node${extension}`);
const password = `Disposable-${randomUUID()}`;
const port = 46242;
const origin = `http://127.0.0.1:${port}`;
let directory: string, child: ChildProcess, addressA: string, addressB: string;

function command(args: string[], secret?: string) {
  const result = spawnSync(cli, args, { encoding: 'utf8', input: secret, windowsHide: true });
  if (result.status !== 0) throw new Error(`Native CLI failed: ${result.stderr}`);
  return result.stdout.trim();
}
async function startNode() {
  child = spawn(binary, ['run', '--db', resolve(directory, 'node.sqlite'), '--listen', `127.0.0.1:${port}`, '--web-dir', resolve(root, 'web/dist')], { windowsHide: true, stdio: 'ignore' });
  await expect.poll(async () => { try { return (await fetch(`${origin}/v1/status`)).ok; } catch { return false; } }).toBe(true);
}
async function stopNode() {
  if (!child || child.exitCode !== null) return;
  await new Promise<void>(resolve => { child.once('exit', () => resolve()); child.kill(); });
}
async function restore(page: Page, name: string, path: string) {
  await page.goto(`${origin}/#/wallets`);
  await page.getByRole('button', { name: 'Restore wallet', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await dialog.getByLabel('Wallet name').fill(name);
  await dialog.getByLabel('Encrypted wallet file').setInputFiles(path);
  await dialog.getByLabel('Backup password').fill(password);
  await dialog.getByRole('button', { name: 'Restore wallet', exact: true }).click();
  await expect(dialog.getByText('Your wallet is ready')).toBeVisible();
  const download = page.waitForEvent('download');
  await dialog.getByRole('button', { name: 'Download encrypted backup' }).click();
  await download;
  await dialog.getByRole('button', { name: 'Continue to wallet' }).click();
}
async function send(page: Page, to: string, amount: string, fee: string) {
  await page.goto(`${origin}/#/send`);
  await page.getByLabel('Recipient address').fill(to);
  await page.getByLabel('Amount', { exact: false }).fill(amount);
  await page.getByLabel('Network fee', { exact: false }).fill(fee);
  await page.getByRole('button', { name: 'Review transfer', exact: true }).click();
  await page.getByLabel('Wallet password', { exact: true }).fill(password);
  await page.getByRole('button', { name: 'Sign & send' }).click();
  await expect(page.getByText('Transfer submitted', { exact: true })).toBeVisible();
  const pool = await (await fetch(`${origin}/v1/explorer/mempool`)).json();
  expect(pool).toHaveLength(1); return pool[0].id as string;
}
async function produce(page: Page) {
  await page.getByRole('button', { name: /Produce (authority )?block/i }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog.getByLabel('Authority wallet', { exact: true })).toHaveValue('Wallet A');
  await dialog.getByLabel('Authority wallet password').fill(password);
  await dialog.getByRole('button', { name: 'Sign & produce block' }).click();
  await expect(dialog).not.toBeVisible();
}
async function capture(page: Page, path: string) {
  await page.locator('main').focus();
  await page.evaluate(async () => {
    const finite = document.getAnimations().filter(a => Number.isFinite(a.effect?.getComputedTiming().endTime));
    await Promise.all(finite.map(a => a.finished.catch(() => undefined)));
  });
  await expect(page.locator('.skip-link')).not.toBeFocused();
  await expect.poll(() => page.locator('.skip-link').evaluate(e => e.getBoundingClientRect().bottom)).toBeLessThanOrEqual(0);
  await page.screenshot({ path, fullPage: true });
}

test.beforeEach(async ({ context }) => { await context.addInitScript(() => { try { if (!localStorage.getItem('orvexa.locale')) localStorage.setItem('orvexa.locale', 'en'); } catch { /* about:blank has no storage origin */ } }); });

test.beforeAll(async () => {
  mkdirSync(resolve(root, 'demo-data'), { recursive: true });
  directory = mkdtempSync(resolve(root, 'demo-data/web-e2e-'));
  addressA = command(['wallet-new', '--wallet', resolve(directory, 'a.wallet.json'), '--password-stdin'], password);
  addressB = command(['wallet-new', '--wallet', resolve(directory, 'b.wallet.json'), '--password-stdin'], password);
  command(['genesis', '--authority', addressA, '--recipient', addressA, '--out', resolve(directory, 'genesis.json')]);
  const init = spawnSync(binary, ['init', '--db', resolve(directory, 'node.sqlite'), '--genesis', resolve(directory, 'genesis.json')], { encoding: 'utf8', windowsHide: true });
  if (init.status !== 0) throw new Error(init.stderr);
  await startNode();
});
test.afterAll(stopNode);

test('native backups, browser signing, two-way transfers, Explorer and restart', async ({ page }, info) => {
  const browserErrors: string[] = []; const leaks: string[] = [];
  page.on('pageerror', e => browserErrors.push(e.message));
  page.on('request', request => { if (request.url().includes(password) || request.postData()?.includes(password) || request.postData()?.includes('ciphertext')) leaks.push('Secret wallet material sent in an HTTP request'); });
  await page.goto(origin);
  await expect(page.getByText('Node connected', { exact: true })).toBeVisible();
  await restore(page, 'Wallet A', resolve(directory, 'a.wallet.json'));
  await restore(page, 'Wallet B', resolve(directory, 'b.wallet.json'));
  await page.getByLabel('Active wallet').selectOption({ label: 'Wallet A' });
  await page.goto(`${origin}/#/dashboard`);
  await expect(page.getByTestId('confirmed-balance')).toContainText('1,000,000');
  const first = await send(page, addressB, '250', '2');
  await page.getByRole('link', { name: 'View in Explorer' }).click();
  await expect(page.locator('.detail-panel .badge')).toHaveText(/Pending/);
  await expect(page.locator('.detail-grid > div').filter({ hasText: 'Confirmations' })).toContainText('0');
  await produce(page);
  await expect(page.locator('.detail-panel .badge')).toHaveText(/Confirmed/);
  await expect(page.locator('.detail-grid > div').filter({ hasText: 'Confirmations' })).toContainText('1');
  await page.getByLabel('Active wallet').selectOption({ label: 'Wallet B' });
  await page.goto(`${origin}/#/dashboard`);
  await expect(page.getByTestId('confirmed-balance')).toContainText('250');
  const second = await send(page, addressA, '40', '1');
  await produce(page);
  await expect(page.getByTestId('confirmed-balance')).toContainText('209');
  await page.goto(`${origin}/#/explorer`);
  await page.getByRole('textbox', { name: 'Search transaction hash, address or block height' }).fill(first);
  await page.getByRole('button', { name: 'Search', exact: true }).click();
  await expect(page.locator('.detail-panel .badge')).toHaveText(/Confirmed/);
  await expect(page.locator('.detail-grid > div').filter({ hasText: 'Confirmations' })).toContainText('2');
  await page.getByRole('link', { name: '#1', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Block #1', exact: true })).toBeVisible();
  await expect(page.locator('.detail-panel .activity-row')).toHaveCount(1);
  await page.goto(`${origin}/#/explorer/address/${addressB}`);
  await expect(page.getByRole('heading', { name: 'Public address', exact: true })).toBeVisible();
  await expect(page.locator('.detail-grid > div').filter({ hasText: 'Confirmed balance' })).toContainText('209 atoms');
  await expect(page.locator('.detail-panel .activity-row')).toHaveCount(2);
  await page.goto(`${origin}/#/transactions`);
  await expect(page.locator('.activity-row')).toHaveCount(2);
  await expect(page.locator('.activity-row').filter({ hasText: 'Received' })).toHaveCount(1);
  await expect(page.locator('.activity-row').filter({ hasText: 'Sent' })).toHaveCount(1);
  await stopNode(); await startNode();
  await page.goto(`${origin}/#/dashboard`);
  await expect(page.getByTestId('confirmed-balance')).toContainText('209');
  const state = await (await fetch(`${origin}/v1/status`)).json();
  const a = await (await fetch(`${origin}/v1/balance/${addressA}`)).json();
  expect(state.height).toBe(2); expect(state.burned_fees).toBe(3); expect(a.confirmed.balance).toBe(999788);
  await page.getByLabel('Active wallet').selectOption({ label: 'Wallet A' });
  await expect(page.getByTestId('confirmed-balance')).toContainText('999,788');
  await page.setViewportSize({ width: 1440, height: 1000 });
  await expect(page.locator('.drawer-close')).toBeHidden();
  await capture(page, info.outputPath('dashboard-desktop.png'));
  for (const route of ['send', 'receive', 'transactions', 'wallets', 'backup', 'settings', 'explorer']) {
    await page.goto(`${origin}/#/${route}`);
    if (route === 'receive') await expect(page.locator('.qr-frame img')).toBeVisible();
    if (route === 'explorer') await expect(page.locator('.block-row')).toHaveCount(2);
    await capture(page, info.outputPath(`${route}-desktop.png`));
  }
  await page.goto(`${origin}/#/dashboard`);
  await expect(page.getByTestId('confirmed-balance')).toContainText('999,788');
  await page.setViewportSize({ width: 390, height: 844 });
  await expect.poll(() => page.locator('.sidebar').evaluate(e => e.getBoundingClientRect().right)).toBeLessThanOrEqual(0);
  await capture(page, info.outputPath('dashboard-mobile.png'));
  const menu = page.locator('button.mobile-menu');
  await menu.click(); await expect(menu).toHaveAttribute('aria-expanded', 'true');
  await expect(page.locator('.sidebar').getByRole('button', { name: 'Close navigation', exact: true })).toBeFocused();
  await page.keyboard.press('Escape'); await expect(menu).toBeFocused();
  await expect(page.locator('#primary-navigation')).toHaveAttribute('inert', '');
  await menu.click();
  await page.getByRole('link', { name: 'Dashboard', exact: true }).click();
  await expect(menu).toHaveAttribute('aria-expanded', 'false');
  for (const route of ['send', 'receive', 'transactions', 'wallets', 'backup', 'settings', 'explorer']) {
    await page.goto(`${origin}/#/${route}`);
    if (route === 'receive') await expect(page.locator('.qr-frame img')).toBeVisible();
    await expect.poll(() => page.locator('.sidebar').evaluate(e => e.getBoundingClientRect().right)).toBeLessThanOrEqual(0);
    await capture(page, info.outputPath(`${route}-mobile.png`));
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  expect(browserErrors).toEqual([]); expect(leaks).toEqual([]);
  writeFileSync(resolve(directory, 'result.json'), JSON.stringify({ passed: true, chain_id: state.chain_id, height: state.height, tip: state.tip, wallet_a_balance: a.confirmed.balance, wallet_b_balance: 209, burned_fees: 3, first, second }, null, 2));
});

test('browser-created backup unlocks with native CLI and receive QR is real', async ({ page }) => {
  await page.goto(`${origin}/#/wallets`);
  await page.getByRole('button', { name: 'Create wallet', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await dialog.getByLabel('Wallet name').fill('Browser wallet');
  await dialog.getByLabel('New password', { exact: true }).fill(password);
  await dialog.getByLabel('Repeat password').fill(password);
  await dialog.getByRole('button', { name: 'Create wallet', exact: true }).click();
  await expect(dialog.getByText('Your wallet is ready')).toBeVisible();
  const downloadPromise = page.waitForEvent('download');
  await dialog.getByRole('button', { name: 'Download encrypted backup' }).click();
  const download = await downloadPromise; const file = resolve(directory, 'browser-created.wallet.json'); await download.saveAs(file);
  const address = command(['wallet-verify', '--wallet', file, '--password-stdin'], password);
  expect(address).toBe(JSON.parse(readFileSync(file, 'utf8')).metadata.address);
  await dialog.getByRole('button', { name: 'Continue to wallet' }).click();
  await page.goto(`${origin}/#/receive`);
  await expect(page.getByRole('img', { name: /QR code for Browser wallet/ })).toBeVisible();
  await expect(page.locator('.hash-value')).toContainText(address);
  await page.goto(`${origin}/#/backup`);
  await page.getByLabel('Encrypted backup file').setInputFiles(file);
  await page.getByLabel('Backup password', { exact: true }).fill('incorrect-password');
  await page.getByRole('button', { name: 'Verify backup', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('wrong password or tampered wallet');
  await page.getByLabel('Backup password', { exact: true }).fill(password);
  await page.getByRole('button', { name: 'Verify backup', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('Backup verified locally');
});

test('unknown transaction is not invented and network mismatch disables signing', async ({ page }) => {
  let releaseWasm!: () => void; let wasmRequested = false;
  const wasmGate = new Promise<void>(resolve => { releaseWasm = resolve; });
  await page.route('**/wasm/orvexa_web_wallet_bg.wasm', async route => {
    wasmRequested = true; await wasmGate; await route.continue();
  });
  await page.goto(`${origin}/#/explorer/tx/${'0'.repeat(64)}`);
  await expect(page.getByRole('alert')).toContainText('transaction not found');
  await expect(page.locator('.detail-panel')).toHaveCount(0);
  await expect.poll(() => wasmRequested).toBe(true);
  // Change the pin while the first worker verification is in flight. This must
  // be preserved even on fast Release builds, without relying on a page reload.
  await page.evaluate(() => localStorage.setItem('orvexa.chain', 'f'.repeat(64)));
  releaseWasm();
  await expect(page.getByRole('alert').filter({ hasText: 'Network identity changed' })).toBeVisible();
  expect(await page.evaluate(() => localStorage.getItem('orvexa.chain'))).toBe('f'.repeat(64));
  await page.goto(`${origin}/#/settings`);
  await expect(page.getByRole('alert').filter({ hasText: 'Network identity changed' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Use observed test network' })).toBeDisabled();
  await page.getByLabel('I verified this chain ID and want to use this test network.').check();
  await page.getByRole('button', { name: 'Use observed test network' }).click();
  await expect(page.getByText('Node connected', { exact: true })).toBeVisible();
  const otherTab = await page.context().newPage();
  await otherTab.goto(`${origin}/#/settings`);
  await expect(otherTab.getByText('Node connected', { exact: true })).toBeVisible();
  await otherTab.evaluate(() => localStorage.setItem('orvexa.chain', 'e'.repeat(64)));
  await expect(page.getByRole('alert').filter({ hasText: 'Network identity changed' })).toBeVisible();
  expect(await page.evaluate(() => localStorage.getItem('orvexa.chain'))).toBe('e'.repeat(64));
  await otherTab.close();
});

test('sync state is explicit and lifts when the node catches up', async ({ page }) => {
  // Peers advertise only authority-signed heads above the local height; fake one far ahead.
  await page.route('**/v1/runtime', route => route.fulfill({ json: { enabled: true, transport: 'libp2p-tcp-noise', peer_id: '', listen_addresses: [], role: 'full', halted: false, produced_blocks: 0,
    peers: [{ url: '/ip4/192.0.2.1/tcp/4243', state: 'connected', height: 999 }] } }));
  await page.goto(`${origin}/#/dashboard`);
  await expect(page.getByText('Syncing', { exact: true })).toBeVisible();
  await page.unroute('**/v1/runtime');
  await expect(page.getByText('Node connected', { exact: true })).toBeVisible({ timeout: 15000 });
});
