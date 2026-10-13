import { afterAll, beforeAll, expect, test } from 'vitest';
import {
  account,
  api,
  eventually,
  sdk,
  startIntegrationStack,
  startWorker,
  type IntegrationStack,
} from './support';
let stack: IntegrationStack;
beforeAll(async () => {
  stack = await startIntegrationStack();
});
afterAll(async () => {
  await stack?.cleanup();
});
test('typing expires without durable events and presence combines multiple devices with peer-only access', async () => {
  const a = await account(stack);
  const b = await account(stack);
  const c = await account(stack);
  const conv = await (
    await api(stack, a.cookie, '/conversations/direct', 'POST', {
      peer_id: b.user.id,
    })
  ).json();
  const ac = sdk(stack, a.cookie),
    b1 = sdk(stack, b.cookie),
    b2 = sdk(stack, b.cookie);
  const events: Record<string, unknown>[] = [];
  try {
    await ac.connect();
    const asub = await ac.subscribe(`personal:#${a.user.id}`);
    asub.on('publication', (ctx) => events.push(ctx.data));
    await b1.connect();
    const bs = await b1.subscribe(`personal:#${b.user.id}`);
    bs.on('publication', (ctx) => events.push(ctx.data));
    await b2.connect();
    await b2.subscribe(`personal:#${b.user.id}`);
    await startWorker(stack);
    expect(
      (await (await api(stack, a.cookie, `/users/${b.user.id}/status`)).json())
        .online,
    ).toBe(true);
    expect(
      (await api(stack, a.cookie, `/users/${c.user.id}/status`)).status,
    ).toBe(404);
    const count = Number(
      (await stack.db.query('SELECT count(*) FROM conversation_events')).rows[0]
        .count,
    );
    expect(
      (
        await api(stack, a.cookie, `/conversations/${conv.id}/typing`, 'POST', {
          is_typing: true,
        })
      ).status,
    ).toBe(204);
    await eventually(async () =>
      events.some((e) => e.kind === 'typing.changed'),
    );
    const typing = events.find((e) => e.kind === 'typing.changed')!;
    expect(typing.event_seq).toBeUndefined();
    expect(
      Date.parse(typing.expires_at as string) - Date.now(),
    ).toBeLessThanOrEqual(5000);
    await eventually(
      async () => Date.parse(typing.expires_at as string) <= Date.now(),
      6000,
    );
    expect(
      Number(
        (await stack.db.query('SELECT count(*) FROM conversation_events'))
          .rows[0].count,
      ),
    ).toBe(count);
    b1.stop();
    expect(
      (await (await api(stack, a.cookie, `/users/${b.user.id}/status`)).json())
        .online,
    ).toBe(true);
    b2.stop();
    await eventually(
      async () =>
        (
          await (
            await api(stack, a.cookie, `/users/${b.user.id}/status`)
          ).json()
        ).online === false,
    );
    await eventually(async () =>
      events.some(
        (e) => e.kind === 'presence.changed' && e.user_id === b.user.id,
      ),
    );
  } finally {
    ac.stop();
    b1.stop();
    b2.stop();
  }
});
