import { defineConfig, devices } from '@playwright/test';
export default defineConfig({
  testDir: './e2e',
  testIgnore: 'deployment.spec.ts',
  fullyParallel: false,
  workers: 1,
  timeout: 30000,
  use: { baseURL: 'http://localhost:5174', trace: 'retain-on-failure' },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  reporter: 'list',
});
