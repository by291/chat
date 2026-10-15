import { afterAll, beforeAll, expect, test } from 'vitest';
import {
  account,
  eventually,
  sdk,
  startIntegrationStack,
  type IntegrationStack,
} from './support';
let stack: IntegrationStack;

beforeAll(async () => {
  stack = await startIntegrationStack();
});

afterAll(async () => {
  await stack?.cleanup();
});

test('Cookie authenticates a real SDK and only its own channel is accessible', async () => {
  const a = await account(stack);
  const b = await account(stack);
  const client = sdk(stack, a.cookie);
  try {
    await client.connect();
    await client.subscribe(`personal:#${a.user.id}`);
    await expect(
      client.subscribe(`personal:#${b.user.id}`),
    ).rejects.toBeDefined();
    await expect(
      client.publish(`personal:#${a.user.id}`, {}),
    ).rejects.toBeDefined();
    await expect(
      client.client.presence(`personal:#${a.user.id}`),
    ).rejects.toBeDefined();
  } finally {
    client.stop();
  }
});

test('revoked Cookie and wrong Origin cannot establish a connection', async () => {
  const a = await account(stack);
  await stack.db.query(
    'UPDATE sessions SET revoked_at=now() WHERE user_id=$1',
    [a.user.id],
  );
  const revoked = sdk(stack, a.cookie);
  const wrong = sdk(stack, a.cookie, 'https://evil.example');
  try {
    await expect(revoked.connect()).rejects.toBeDefined();
    await expect(wrong.connect()).rejects.toBeDefined();
  } finally {
    revoked.stop();
    wrong.stop();
  }
});

test('server-side refresh meta rejects an expired database session within the 60 second lease', async () => {
  const a = await account(stack);
  const client = sdk(stack, a.cookie);
  let disconnected = false;
  try {
    await client.connect();
    await client.subscribe(`personal:#${a.user.id}`);
    client.client.on('disconnected', () => {
      disconnected = true;
    });
    const started = Date.now();
    await stack.db.query(
      "UPDATE sessions SET expires_at=now()-interval '1 second' WHERE user_id=$1",
      [a.user.id],
    );
    await eventually(async () => disconnected, 65000);
    expect(Date.now() - started).toBeLessThan(65000);
    console.info(
      'Measured session expiry disconnect ms:',
      Date.now() - started,
    );
  } finally {
    client.stop();
  }
}, 70000);
