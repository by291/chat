import { defineConfig, devices } from '@playwright/test';

const origin = process.env.CHAT_DEPLOYMENT_ORIGIN;
const mediaOrigin = process.env.CHAT_DEPLOYMENT_MEDIA_ORIGIN;
if (!origin || !mediaOrigin) {
  throw new Error(
    'Configure the HTTPS staging origins described in docs/operations/deployment.md',
  );
}
const app = new URL(origin);
const media = new URL(mediaOrigin);
if (
  app.protocol !== 'https:' ||
  media.protocol !== 'https:' ||
  app.origin === media.origin ||
  app.username ||
  app.password ||
  media.username ||
  media.password
) {
  throw new Error(
    'Deployment tests require separate HTTPS application and media origins',
  );
}

export default defineConfig({
  testDir: './e2e',
  testMatch: 'deployment.spec.ts',
  outputDir: './test-results-deployment',
  workers: 1,
  timeout: 30000,
  use: {
    ignoreHTTPSErrors: process.env.CHAT_DEPLOYMENT_INSECURE_TLS === 'true',
    baseURL: app.origin,
    trace: 'retain-on-failure',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  reporter: 'list',
});
