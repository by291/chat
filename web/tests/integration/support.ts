import { createHash, randomBytes } from 'node:crypto';

import WS from 'ws';
import { Centrifuge } from 'centrifuge';

import { TestRuntime } from './runtime';

export type IntegrationStack = TestRuntime;

export async function startIntegrationStack(): Promise<IntegrationStack> {
  return TestRuntime.start();
}

export async function account(stack: IntegrationStack) {
  const token = randomBytes(32).toString('hex');
  const username = `u${randomBytes(8).toString('hex')}`;
  await stack.db.query(
    "INSERT INTO invitations(id,token_hash,expires_at) VALUES(gen_random_uuid(),$1,now()+interval '7 days')",
    [createHash('sha256').update(token).digest('hex')],
  );
  const response = await fetch(`${stack.apiUrl}/api/v1/auth/register`, {
    method: 'POST',
    headers: { Origin: stack.origin, 'Content-Type': 'application/json' },
    body: JSON.stringify({
      invitation: token,
      username,
      display_name: username,
      password: 'correct horse battery',
    }),
  });
  if (response.status !== 201)
    throw new Error(`Registration failed: ${response.status}`);
  const user = (await response.json()) as {
    id: string;
    username: string;
    display_name: string;
  };
  const cookie = response.headers.get('set-cookie')!.split(';')[0];
  return { user, cookie };
}
export function sdk(
  stack: IntegrationStack,
  cookie: string,
  origin = stack.origin,
) {
  class Socket extends WS {
    constructor(url: string, protocols?: string | string[]) {
      super(url, protocols, { headers: { Cookie: cookie, Origin: origin } });
    }
  }
  const client = new Centrifuge(stack.wsUrl, {
    websocket: Socket as unknown as typeof WebSocket,
    timeout: 3000,
  });
  client.on('disconnected', (ctx) =>
    console.error('SDK disconnected', ctx.code, ctx.reason),
  );
  client.on('error', (ctx) =>
    console.error('SDK error', ctx.type, ctx.error.code, ctx.error.message),
  );
  return {
    client,
    connect: async () => {
      client.connect();
      await client.ready(5000);
    },
    subscribe: async (channel: string) => {
      const sub = client.newSubscription(channel);
      sub.subscribe();
      await sub.ready(3000);
      return sub;
    },
    publish: (channel: string, data: unknown) => client.publish(channel, data),
    stop: () => client.disconnect(),
  };
}
export async function startWorker(
  stack: IntegrationStack,
  pauseAfterPublish = false,
) {
  await stack.startWorker(pauseAfterPublish);
}

export async function api(
  stack: IntegrationStack,
  cookie: string,
  path: string,
  method = 'GET',
  body?: unknown,
) {
  return fetch(`${stack.apiUrl}/api/v1${path}`, {
    method,
    headers: {
      Cookie: cookie,
      Origin: stack.origin,
      'Content-Type': 'application/json',
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}
export async function eventually(
  check: () => Promise<boolean>,
  timeout = 10000,
) {
  const until = Date.now() + timeout;
  while (Date.now() < until) {
    if (await check()) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error('Expected condition did not become true');
}
