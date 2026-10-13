import { createServer, type Server } from 'node:http';
import { afterAll, beforeAll, expect, test } from 'vitest';
import { createApiClient } from '../../src/lib/api-client';
import { SessionState } from '../../src/lib/features/auth/session.svelte';
let server: Server;
let baseUrl: string;
let status = 200;
const user = {
  id: '01999cee-0000-7000-8000-000000000001',
  username: 'alice',
  display_name: 'Alice',
};
beforeAll(async () => {
  server = createServer((_req, res) => {
    res.writeHead(status, { 'Content-Type': 'application/json' });
    res.end(
      JSON.stringify(
        status === 200
          ? user
          : { code: 'unauthorized', message: '请重新登录', request_id: 'test' },
      ),
    );
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const addr = server.address();
  baseUrl = `http://127.0.0.1:${typeof addr === 'object' && addr ? addr.port : 0}`;
});
afterAll(() => server.close());
test('HTTP 401 clears the restored identity and notifies chat cache owners', async () => {
  const session = new SessionState(createApiClient({ baseUrl }));
  let cacheCleared = false;
  session.onReset(() => {
    cacheCleared = true;
  });
  await session.restore();
  expect(session.user?.id).toBe(user.id);
  expect(session.pending).toBe(false);
  status = 401;
  await session.restore();
  expect(session.user).toBeUndefined();
  expect(cacheCleared).toBe(true);
});
