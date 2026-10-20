import { randomBytes } from 'node:crypto';

import { Centrifuge } from 'centrifuge';
import WS from 'ws';

import { TestRuntime } from './runtime';

export type IntegrationStack = TestRuntime;

export async function startIntegrationStack(): Promise<IntegrationStack> {
  return TestRuntime.start();
}

export async function account(stack: IntegrationStack) {
  const username = `u${randomBytes(8).toString('hex')}`;

  const response = await fetch(`${stack.apiUrl}/api/v1/auth/register`, {
    method: 'POST',
    headers: { Origin: stack.origin, 'Content-Type': 'application/json' },
    body: JSON.stringify({
      username,
      display_name: username,
      password: 'correct horse battery',
    }),
  });
  if (response.status !== 201) {
    throw new Error(`Registration failed: ${response.status}`);
  }

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
  client.on('disconnected', (event) =>
    console.error('SDK disconnected', event.code, event.reason),
  );
  client.on('error', (event) =>
    console.error(
      'SDK error',
      event.type,
      event.error.code,
      event.error.message,
    ),
  );

  return {
    client,
    connect: async () => {
      client.connect();
      await client.ready(5000);
    },
    subscribe: async (channel: string) => {
      const subscription = client.newSubscription(channel);
      subscription.subscribe();
      await subscription.ready(3000);

      return subscription;
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
  const deadline = Date.now() + timeout;

  while (Date.now() < deadline) {
    if (await check()) {
      return;
    }

    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  throw new Error('Expected condition did not become true');
}
