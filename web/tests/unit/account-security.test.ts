import { createServer, type ServerResponse } from 'node:http';

import { afterEach, expect, test } from 'vitest';

import { createApiClient, ApiFailure } from '../../src/lib/api-client';
import { SessionState } from '../../src/lib/features/auth/session.svelte';

const alice = {
  id: '01999cee-0000-7000-8000-000000000001',
  username: 'alice',
  display_name: 'Alice',
};
const bob = {
  id: '01999cee-0000-7000-8000-000000000002',
  username: 'bob',
  display_name: 'Bob',
};
const passwordInput = {
  current_password: 'correct horse battery',
  new_password: 'new correct password',
};
const cleanup: (() => Promise<void>)[] = [];

afterEach(async () => {
  for (const close of cleanup.splice(0)) await close();
});

async function fixture(status = 204, delayed = false) {
  let currentIdentity = alice;
  let passwordResponse: ServerResponse | undefined;
  let acknowledgeRequest!: () => void;
  const passwordRequested = new Promise<void>((resolve) => {
    acknowledgeRequest = resolve;
  });
  let submittedPassword: unknown;
  const server = createServer(async (request, response) => {
    if (request.url === '/api/v1/auth/password') {
      let body = '';
      for await (const chunk of request) body += chunk;
      submittedPassword = JSON.parse(body);
      passwordResponse = response;
      acknowledgeRequest();
      if (delayed) return;
      finishPassword();
      return;
    }
    response.writeHead(200, { 'Content-Type': 'application/json' });
    response.end(
      JSON.stringify(
        request.url === '/api/v1/auth/login' ? bob : currentIdentity,
      ),
    );
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  if (!address || typeof address === 'string')
    throw new Error('Missing fixture address');
  const session = new SessionState(
    createApiClient({ baseUrl: `http://127.0.0.1:${address.port}` }),
  );
  cleanup.push(
    () =>
      new Promise<void>((resolve) => {
        server.closeAllConnections();
        server.close(() => resolve());
      }),
  );
  await session.restore();

  function finishPassword() {
    passwordResponse!.writeHead(status, { 'Content-Type': 'application/json' });
    passwordResponse!.end(
      status === 204
        ? undefined
        : JSON.stringify({
            code: status === 400 ? 'invalid_current_password' : 'unauthorized',
            message: status === 400 ? '当前密码不正确' : '请重新登录',
            request_id: 'owned-fixture',
          }),
    );
  }

  return {
    session,
    passwordRequested,
    finishPassword,
    submitted: () => submittedPassword,
    restoreAsBob: () => {
      currentIdentity = bob;
    },
  };
}

test('incorrect current password preserves the authenticated identity and cache lifetime', async () => {
  const { session } = await fixture(400);
  let resets = 0;
  session.onReset(() => resets++);

  await expect(session.changePassword(passwordInput)).rejects.toMatchObject({
    status: 400,
    code: 'invalid_current_password',
  });

  expect(session.user).toEqual(alice);
  expect(resets).toBe(0);
});

test('successful password change submits the contract and clears session-owned state', async () => {
  const { session, submitted } = await fixture();
  let resets = 0;
  session.onReset(() => resets++);

  await session.changePassword(passwordInput);

  expect(submitted()).toEqual(passwordInput);
  expect(session.user).toBeUndefined();
  expect(session.pending).toBe(false);
  expect(resets).toBe(1);
});

test('late password success from an old lifetime cannot sign out a new identity', async () => {
  const { session, passwordRequested, finishPassword } = await fixture(
    204,
    true,
  );
  const pending = session.changePassword(passwordInput);
  await passwordRequested;
  await session.login({
    username: 'bob',
    password: 'bob password',
    device_label: 'Browser',
  });

  finishPassword();
  await pending;

  expect(session.user).toEqual(bob);
});

test('late unauthorized password response cannot discard a newer authenticated identity', async () => {
  const { session, passwordRequested, finishPassword } = await fixture(
    401,
    true,
  );
  const pending = session
    .changePassword(passwordInput)
    .catch((error: unknown) => error);
  await passwordRequested;
  await session.login({
    username: 'bob',
    password: 'bob password',
    device_label: 'Browser',
  });

  finishPassword();

  expect(await pending).toBeInstanceOf(ApiFailure);
  expect(session.user).toEqual(bob);
});

test('restoring a different identity fences pending requests from the previous account', async () => {
  const { session, passwordRequested, finishPassword, restoreAsBob } =
    await fixture(401, true);
  const pending = session
    .changePassword(passwordInput)
    .catch((error: unknown) => error);
  await passwordRequested;
  restoreAsBob();
  await session.restore();

  finishPassword();
  await pending;

  expect(session.user).toEqual(bob);
  expect(session.pending).toBe(false);
});
