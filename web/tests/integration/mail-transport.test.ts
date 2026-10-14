import { expect, test } from 'vitest';

import { TestSmtpServer } from './mail-support';
import { TestRuntime } from './runtime';
import { account, api } from './support';

test('native startup accepts an owned mail transport while text authentication remains usable', async () => {
  const mail = new TestSmtpServer();
  let runtime: TestRuntime | undefined;
  await mail.start();
  try {
    runtime = await TestRuntime.start({ mail: mail.environment() });
    const actor = await account(runtime);

    const response = await api(runtime, actor.cookie, '/auth/me');

    expect(response.status).toBe(200);
    expect((await response.json()).id).toBe(actor.user.id);
    expect(runtime.env.MAIL_ENABLED).toBe('true');
    expect(mail.messages()).toEqual([]);
  } finally {
    await runtime?.cleanup();
    await mail.stop();
  }
}, 120000);

test('invalid enabled SMTP configuration rejects native startup without leaking credentials', async () => {
  const mail = new TestSmtpServer();
  await mail.start();
  try {
    const environment = mail.environment();
    const error = await TestRuntime.start({
      mail: { ...environment, SMTP_TLS_MODE: 'opportunistic' },
    }).then(
      async (runtime) => {
        await runtime.cleanup();
        return undefined;
      },
      (reason: unknown) => reason,
    );

    expect(error).toBeInstanceOf(Error);
    expect(String(error)).toContain('邮件服务配置无效');
    expect(String(error)).not.toContain(environment.SMTP_PASSWORD);
    expect(String(error)).not.toContain(environment.MAIL_QUEUE_ENCRYPTION_KEY);
  } finally {
    await mail.stop();
  }
}, 120000);
