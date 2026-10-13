import { test, expect } from 'vitest';
import { spawn } from 'node:child_process';
import { readFile, copyFile, mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { randomUUID } from 'node:crypto';
import { startIntegrationStack, account, api, eventually } from './support';
const root = fileURLToPath(new URL('../../../', import.meta.url));
async function command(exe: string, args: string[]) {
  return new Promise<void>((resolve, reject) => {
    const child = spawn(exe, args, {
      cwd: root,
      stdio: 'ignore',
      windowsHide: true,
    });
    child.once('error', reject);
    child.once('exit', (code) =>
      code === 0
        ? resolve()
        : reject(new Error(`${exe} operation failed: ${code}`)),
    );
  });
}
const parseEnv = (text: string) =>
  Object.fromEntries(
    text
      .split(/\r?\n/)
      .filter((l) => /^[A-Z_]+=/.test(l))
      .map((line) => {
        const at = line.indexOf('=');
        return [line.slice(0, at), line.slice(at + 1)];
      }),
  );
test('backup restores accounts, messages and revoked sessions into a fresh isolated project', async () => {
  const stack = await startIntegrationStack();
  const suffix = randomUUID().replaceAll('-', '');
  const project = `chat-restore-drill-${suffix}`;
  const backup = `${root}/backups/drill-${suffix}`;
  let restored: ReturnType<typeof spawn> | undefined;
  let restoreEnv = '';
  try {
    const a = await account(stack),
      b = await account(stack);
    const conversation = await (
      await api(stack, a.cookie, '/conversations/direct', 'POST', {
        peer_id: b.user.id,
      })
    ).json();
    await api(
      stack,
      a.cookie,
      `/conversations/${conversation.id}/messages`,
      'POST',
      { client_message_id: randomUUID(), body: '恢复演练：完整消息 🌱' },
    );
    expect((await api(stack, a.cookie, '/auth/logout', 'POST')).status).toBe(
      204,
    );
    const source = parseEnv(await readFile(stack.envFile, 'utf8'));
    const database = new URL(source.DATABASE_URL).pathname.slice(1);
    await command('pwsh', [
      '-NoProfile',
      '-File',
      'scripts/backup.ps1',
      '-OutputDirectory',
      backup,
      '-ProjectName',
      'chat-e2e',
      '-EnvFile',
      stack.envFile,
      '-ComposeFile',
      'infra/compose/compose.dev.yaml',
      '-DatabaseName',
      database,
    ]);
    await command('pwsh', [
      '-NoProfile',
      '-File',
      'scripts/restore.ps1',
      '-BackupDirectory',
      backup,
      '-ProjectName',
      project,
    ]);
    restoreEnv = `${root}/backups/restore-state/${project}/restore.env`;
    const settings = parseEnv(await readFile(restoreEnv, 'utf8'));
    await mkdir(`${backup}/runtime`, { recursive: true });
    await copyFile(`${root}/target/debug/api.exe`, `${backup}/runtime/api.exe`);
    restored = spawn(`${backup}/runtime/api.exe`, [], {
      cwd: root,
      windowsHide: true,
      stdio: 'ignore',
      env: {
        ...process.env,
        ...source,
        DATABASE_URL: `postgres://${settings.RESTORE_DB_USER}:${settings.RESTORE_PASSWORD}@127.0.0.1:${settings.RESTORE_PORT}/${settings.RESTORE_DB_NAME}`,
        BIND_ADDR: '127.0.0.1:19090',
        INTERNAL_BIND_ADDR: '127.0.0.1:19091',
      },
    });
    await eventually(async () => {
      try {
        return (await fetch('http://127.0.0.1:19090/health/ready')).ok;
      } catch {
        return false;
      }
    }, 15000);
    expect(
      (
        await fetch('http://127.0.0.1:19090/api/v1/auth/me', {
          headers: { Cookie: a.cookie },
        })
      ).status,
    ).toBe(401);
    const login = await fetch('http://127.0.0.1:19090/api/v1/auth/login', {
      method: 'POST',
      headers: { Origin: stack.origin, 'Content-Type': 'application/json' },
      body: JSON.stringify({
        username: a.user.username,
        password: 'correct horse battery',
        device_label: 'restore verification',
      }),
    });
    expect(login.status).toBe(200);
    const cookie = login.headers.get('set-cookie')!.split(';')[0];
    const history = await fetch(
      `http://127.0.0.1:19090/api/v1/conversations/${conversation.id}/messages`,
      { headers: { Cookie: cookie } },
    );
    expect(history.status).toBe(200);
    expect(
      (await history.json()).items.map((m: { body: string }) => m.body),
    ).toContain('恢复演练：完整消息 🌱');
  } finally {
    if (restored) {
      restored.kill();
    }
    if (restoreEnv)
      await command('docker', [
        'compose',
        '--project-name',
        project,
        '--env-file',
        restoreEnv,
        '--file',
        'infra/compose/compose.restore.yaml',
        'down',
        '--volumes',
      ]);
    await stack.cleanup();
  }
}, 120000);
