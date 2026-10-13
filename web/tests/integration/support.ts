import { execFile, spawn } from 'node:child_process';
import { promisify } from 'node:util';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { createHash, randomBytes } from 'node:crypto';
import pg from 'pg';
import WS from 'ws';
import { Centrifuge } from 'centrifuge';
const exec = promisify(execFile);
const root = fileURLToPath(new URL('../../../', import.meta.url));
export type IntegrationStack = {
  apiUrl: string;
  wsUrl: string;
  origin: string;
  apiPid: number;
  workerPid: number | null;
  envFile: string;
  db: pg.Client;
  cleanup: () => Promise<void>;
};
export async function startIntegrationStack(): Promise<IntegrationStack> {
  // On Windows a background process can inherit captured pipe handles; wait for the launcher exit, then read its state file.
  await new Promise<void>((resolve, reject) => {
    const child = spawn(
      'pwsh',
      ['-NoProfile', '-File', `${root}/scripts/e2e.ps1`, '-Prepare'],
      { cwd: root, stdio: 'ignore', windowsHide: true },
    );
    const timer = setTimeout(() => {
      child.kill();
      reject(new Error('Test stack preparation timed out'));
    }, 120000);
    child.once('error', reject);
    child.once('exit', (code) => {
      clearTimeout(timer);
      code === 0
        ? resolve()
        : reject(
            new Error(
              'Test stack preparation failed; inspect integration logs',
            ),
          );
    });
  });
  const state = JSON.parse(
    await readFile(
      `${root}/.superpowers/sdd/2026-10-12-phase-1-private-chat/integration/stack.json`,
      'utf8',
    ),
  );
  const env = await readFile(state.envFile, 'utf8');
  const url = env
    .split(/\r?\n/)
    .find((line) => line.startsWith('DATABASE_URL='))!
    .slice(13);
  const db = new pg.Client({ connectionString: url });
  await db.connect();
  return {
    ...state,
    db,
    cleanup: async () => {
      await db.end();
      await exec(
        'pwsh',
        ['-NoProfile', '-File', `${root}/scripts/e2e.ps1`, '-Cleanup'],
        { cwd: root },
      );
    },
  };
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
  await new Promise<void>((resolve, reject) => {
    const args = [
      '-NoProfile',
      '-File',
      `${root}/scripts/e2e.ps1`,
      '-StartWorker',
    ];
    if (pauseAfterPublish) args.push('-PauseAfterPublish');
    const child = spawn('pwsh', args, {
      cwd: root,
      stdio: 'ignore',
      windowsHide: true,
    });
    child.once('error', reject);
    child.once('exit', (code) =>
      code === 0 ? resolve() : reject(new Error('Worker launcher failed')),
    );
  });
  const state = JSON.parse(
    await readFile(
      `${root}/.superpowers/sdd/2026-10-12-phase-1-private-chat/integration/stack.json`,
      'utf8',
    ),
  );
  stack.workerPid = state.workerPid;
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
