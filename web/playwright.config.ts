import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  testMatch: '**/*.spec.ts',
  workers: 1,
  timeout: 180000,
  expect: { timeout: 20000 },
  use: { browserName: 'chromium', headless: true, trace: 'off', screenshot: 'only-on-failure' },
  reporter: 'list',
});
