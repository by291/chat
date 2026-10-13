import { test as base } from '@playwright/test';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { createHash, randomBytes } from 'node:crypto';
import {
  startIntegrationStack,
  type IntegrationStack,
} from '../tests/integration/support';
export const test = base.extend<{ stack: IntegrationStack }>({
  stack: [
    async ({}, use) => {
      const stack = await startIntegrationStack();
      try {
        await new Promise<void>((resolve, reject) => {
          const root = fileURLToPath(new URL('../../', import.meta.url));
          const child = spawn(
            'pwsh',
            ['-NoProfile', '-File', `${root}/scripts/e2e.ps1`, '-StartWeb'],
            { cwd: root, stdio: 'ignore', windowsHide: true },
          );
          child.once('error', reject);
          child.once('exit', (code) =>
            code === 0 ? resolve() : reject(new Error('Web launcher failed')),
          );
        });
        await use(stack);
      } finally {
        await stack.cleanup();
      }
    },
    { timeout: 120000 },
  ],
});
export { expect } from '@playwright/test';
export async function invitation(stack: IntegrationStack) {
  const token = randomBytes(32).toString('hex');
  await stack.db.query(
    "INSERT INTO invitations(id,token_hash,expires_at) VALUES(gen_random_uuid(),$1,now()+interval '7 days')",
    [createHash('sha256').update(token).digest('hex')],
  );
  return token;
}
export async function signup(
  page: import('@playwright/test').Page,
  stack: IntegrationStack,
) {
  const username = `e${randomBytes(6).toString('hex')}`,
    password = 'correct horse battery';
  await page.goto(`/register?invite=${await invitation(stack)}`);
  await page.getByLabel('用户名', { exact: true }).fill(username);
  await page.getByLabel('昵称', { exact: true }).fill('测试用户 🌟');
  await page.getByLabel('密码', { exact: true }).fill(password);
  await page.getByRole('button', { name: '创建账号', exact: true }).click();
  await base.expect(page).toHaveURL(/\/app/);
  return { username, password };
}
