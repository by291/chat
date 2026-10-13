import { defineConfig, devices } from '@playwright/test';
export default defineConfig({
  testDir: './e2e',
  testMatch: 'deployment.spec.ts',
  workers: 1,
  timeout: 30000,
  use: {
    ignoreHTTPSErrors: true,
    baseURL: 'https://localhost:5443',
    trace: 'retain-on-failure',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  reporter: 'list',
});
