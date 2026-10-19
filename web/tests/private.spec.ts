import { test, expect, type Page } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync, mkdtempSync } from 'node:fs';
import { resolve } from 'node:path';
import { randomBytes } from 'node:crypto';

/** Opens an app dropdown by its label and picks an option, like a person would. */
async function pick(page: Page, label: string, option: string | RegExp) {
  await page.getByRole('combobox', { name: label, exact: true }).click();
  await page.getByRole('option', { name: option }).click();
}


// Private v2 inside the real ORVEXA Desktop controller (headless). The page
// gets the per-launch session exactly as the native window would.
const root = resolve(import.meta.dirname, '../..');
const bin = resolve(root, process.env.ORVEXA_BIN_DIR || 'target/debug');
const app = resolve(bin, `orvexa-desktop${process.platform === 'win32' ? '.exe' : ''}`);
const origin = 'http://127.0.0.1:59252';
const session = randomBytes(32).toString('hex');
const password = 'disposable password 2026';
let child: ChildProcess; let directory: string;

test.beforeAll(async () => {
  mkdirSync(resolve(root, 'demo-data'), { recursive: true }); directory = mkdtempSync(resolve(root, 'demo-data/private-e2e-'));
  const args = ['--headless', '--listen', '127.0.0.1:59252', '--data-dir', resolve(directory, 'app')];
  if (!process.env.ORVEXA_TEST_EMBEDDED) args.push('--web-dir', resolve(root, 'web/dist'));
  child = spawn(app, args, { stdio: 'ignore', windowsHide: true, env: { ...process.env, ORVEXA_V2_SESSION: session } });
  await expect.poll(async () => { try { return (await fetch(`${origin}/v1/desktop/status`)).ok; } catch { return false; } }).toBe(true);
});
test.afterAll(async () => { if (child && child.exitCode === null) await new Promise<void>(r => { child.once('exit', () => r()); child.kill(); }); });

async function withSession(page: Page) {
  await page.addInitScript(token => { Object.defineProperty(window, '__ORVEXA_V2_SESSION', { value: token, writable: false }); }, session);
}

async function createWallet(page: Page, name: string) {
  await page.getByRole('button', { name: 'Новый кошелёк v2' }).click();
  const dialog = page.getByRole('dialog');
  await dialog.getByLabel('Название кошелька').fill(name);
  await dialog.getByRole('button', { name: 'Показать фразу восстановления' }).click();
  const items = dialog.getByRole('list', { name: 'Фраза восстановления' }).getByRole('listitem');
  await expect(items).toHaveCount(24);
  const words = (await items.allInnerTexts()).map(text => text.replace(/^\s*\d+\s*/, '').trim());
  await dialog.getByLabel('Я записал(а) все 24 слова по порядку').check();
  await dialog.getByRole('button', { name: 'Проверить слова' }).click();
  for (const label of await dialog.getByText(/^Слово №\d+$/).allInnerTexts()) {
    const index = Number(label.replace(/\D/g, '')) - 1;
    await dialog.getByLabel(label, { exact: true }).fill(words[index]);
  }
  await dialog.getByLabel('Новый пароль').fill(password);
  await dialog.getByLabel('Повторите пароль').fill(password);
  await dialog.getByRole('button', { name: 'Создать кошелёк' }).click();
  await expect(dialog).toBeHidden({ timeout: 60000 });
  return words;
}

const balance = (page: Page) => page.getByTestId('v2-balance');
const digits = (text: string) => text.replace(/\D/g, '');

test('private v2 wallet, network, transfer, receipt and viewing key in the desktop app', async ({ page, browser }) => {
  test.setTimeout(240000);
  const errors: string[] = []; page.on('pageerror', e => errors.push(e.message));
  await withSession(page);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto(`${origin}/#/private`);
  await expect(page.getByRole('link', { name: 'Приватный кошелёк' })).toBeVisible();
  // In the app the v1 network is frozen: no v1 sending, only viewing.
  await expect(page.getByRole('link', { name: 'Отправить', exact: true })).toHaveCount(0);
  await expect(page.getByText('Старая сеть v1 · только просмотр')).toBeVisible();
  await expect(page.getByText('Создайте первый приватный кошелёк')).toBeVisible();

  await createWallet(page, 'Основной');
  await page.getByRole('button', { name: 'Создать сеть для «Основной»' }).click();
  await expect.poll(async () => digits(await balance(page).innerText()), { timeout: 60000 }).toBe('1000000');
  await page.screenshot({ path: resolve(directory, 'private-overview.png'), fullPage: true });

  await createWallet(page, 'Второй');
  await expect(page.getByRole('combobox', { name: 'Приватный кошелёк', exact: true })).toBeVisible();
  const second = page.locator('.network-card .hash-value code');
  await expect(second).toHaveText(/^ovxt1/, { timeout: 60000 });
  const address = await second.innerText();

  await pick(page, 'Приватный кошелёк', /^Основной/);
  await expect.poll(async () => digits(await balance(page).innerText()), { timeout: 30000 }).toBe('1000000');
  await page.locator('.tabbar').getByRole('link', { name: 'Отправить' }).click();
  await page.getByLabel('Сумма').fill('250000');
  // The recommended fee follows the amount by itself.
  await expect(page.getByText('Рекомендовано для этого перевода: 10 атомов (действий в переводе: 2).')).toBeVisible();
  await expect(page.getByLabel('Сетевая комиссия', { exact: false })).toHaveValue('10');
  await page.getByLabel('Адрес получателя').fill(address);
  await page.getByRole('button', { name: 'Проверить перевод' }).click();
  await expect(page.getByText('Приватный', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Доказать и отправить' }).click();
  await expect(page.getByRole('heading', { name: 'Приватный перевод отправлен' })).toBeVisible({ timeout: 60000 });
  await page.getByRole('button', { name: 'Готово' }).click();

  await pick(page, 'Приватный кошелёк', /^Второй/);
  await page.locator('.tabbar').getByRole('link', { name: 'Обзор' }).click();
  await expect.poll(async () => digits(await balance(page).innerText()), { timeout: 60000 }).toBe('250000');

  // Receipt from the sender's history, checked on the Receipts page.
  await pick(page, 'Приватный кошелёк', /^Основной/);
  await page.locator('.tabbar').getByRole('link', { name: 'История' }).click();
  await expect(page.getByText('Отправлено')).toBeVisible({ timeout: 30000 });
  await page.locator('.activity-row', { hasText: 'Отправлено' }).getByRole('button', { name: 'Квитанция' }).click();
  const receipt = await page.locator('.hash-value code').first().innerText();
  expect(receipt).toMatch(/^orvexa-receipt-v2:/);
  await page.locator('.tabbar').getByRole('link', { name: 'Квитанции' }).click();
  await page.getByLabel('Квитанция').fill(receipt);
  await page.getByRole('button', { name: 'Проверить квитанцию' }).click();
  await expect(page.locator('.verified-result')).toContainText(address);
  await expect(page.locator('.verified-result')).toContainText(/250\s?000/);

  // Viewing key needs the password again and shows its scope first.
  await page.locator('.tabbar').getByRole('link', { name: 'Фраза и ключи' }).click();
  await expect(page.getByText('не показывает исходящие платежи, сдачу и баланс')).toBeVisible();
  await page.getByLabel('Пароль кошелька').fill('wrong password here');
  await page.getByRole('button', { name: 'Экспортировать ключ просмотра' }).click();
  await expect(page.getByText('Неверный пароль.')).toBeVisible({ timeout: 30000 });
  await page.getByLabel('Пароль кошелька').fill(password);
  await page.getByRole('button', { name: 'Экспортировать ключ просмотра' }).click();
  await expect(page.locator('.hash-value code').last()).toHaveText(/^orvexa-ivk-v2:/, { timeout: 30000 });

  // Explorer v2 lists blocks without amounts.
  await page.locator('.tabbar').getByRole('link', { name: 'Обозреватель v2' }).click();
  await expect(page.locator('.block-row').first()).toBeVisible({ timeout: 30000 });
  await page.screenshot({ path: resolve(directory, 'private-explorer.png'), fullPage: true });
  expect(await page.locator('main').innerText()).not.toContain('250000');

  // A tab without the app session reaches no private wallet.
  const plain = await browser.newPage();
  await plain.goto(`${origin}/#/private`);
  await expect(plain.getByRole('heading', { name: 'Откройте приложение ORVEXA' })).toBeVisible();
  expect((await fetch(`${origin}/v2/desktop/state`)).status).toBe(401);
  await plain.close();
  expect(errors).toEqual([]);
});
