import { test, expect } from 'vitest';
import { spawn } from 'node:child_process';
import { readFile, copyFile, mkdir, writeFile } from 'node:fs/promises';
import { openSync, closeSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { randomUUID } from 'node:crypto';
import {
  startIntegrationStack,
  account,
  api,
  eventually,
  startWorker,
} from './support';
import { readyUpload, sendAttachments } from './media-support';
const root = fileURLToPath(new URL('../../../', import.meta.url));
async function command(exe: string, args: string[]) {
  const logPath = `${root}/.superpowers/sdd/2026-10-13-seaweedfs-media/recovery-${exe}-${randomUUID()}.log`;
  const log = openSync(logPath, 'w');
  return new Promise<void>((resolve, reject) => {
    const child = spawn(exe, args, {
      cwd: root,
      stdio: ['ignore', log, log],
      windowsHide: true,
    });
    closeSync(log);
    child.once('error', reject);
    child.once('exit', (code) =>
      code === 0
        ? resolve()
        : reject(
            new Error(`${exe} operation failed: ${code}; inspect ${logPath}`),
          ),
    );
  });
}
const parseEnv = (text: string) =>
  Object.fromEntries(
    text
      .split(/\r?\n/)
      .filter((l) => /^[A-Z][A-Z0-9_]*=/.test(l))
      .map((line) => {
        const at = line.indexOf('=');
        return [line.slice(0, at), line.slice(at + 1)];
      }),
  );
test('coordinated backup restores media bytes, avatars, queued uploads and revoked sessions into an isolated project', async () => {
  const stack = await startIntegrationStack();
  const suffix = randomUUID().replaceAll('-', '');
  const project = `chat-restore-drill-${suffix}`;
  const backup = `${root}/backups/drill-${suffix}`;
  let restored: ReturnType<typeof spawn> | undefined;
  let restoredWorker: ReturnType<typeof spawn> | undefined;
  let restoreEnv = '';
  try {
    const a = await account(stack),
      b = await account(stack);
    const conversation = await (
      await api(stack, a.cookie, '/conversations/direct', 'POST', {
        peer_id: b.user.id,
      })
    ).json();
    await startWorker(stack);
    const picture = Uint8Array.from(
      await readFile(new URL('../fixtures/sample.png', import.meta.url)),
    );
    const image = await readyUpload(stack, a.cookie, picture, {
      purpose: 'message',
      conversation_id: conversation.id,
      filename: 'sample.png',
      content_type: 'image/png',
    });
    const fileBytes = new TextEncoder().encode('restored file bytes');
    const file = await readyUpload(stack, a.cookie, fileBytes, {
      purpose: 'message',
      conversation_id: conversation.id,
      filename: 'notes.txt',
      content_type: 'application/octet-stream',
    });
    await sendAttachments(stack, a.cookie, conversation.id, [
      image.id,
      file.id,
    ]);
    const avatar = await readyUpload(stack, a.cookie, picture, {
      purpose: 'avatar',
      filename: 'avatar.png',
      content_type: 'image/png',
    });
    expect(
      (
        await api(stack, a.cookie, '/users/me/avatar', 'PUT', {
          attachment_id: avatar.id,
        })
      ).status,
    ).toBe(204);
    const sourceGrant = await (
      await api(
        stack,
        a.cookie,
        `/media/${image.id}/download?variant=thumbnail`,
      )
    ).json();
    const thumbnail = new Uint8Array(
      await (await fetch(sourceGrant.url)).arrayBuffer(),
    );

    // Stop only the owned idle test worker, then seal a genuine queued upload.
    // Recovery must process it rather than relying on a fabricated ready row.
    const stateFile = `${root}/.superpowers/sdd/2026-10-12-phase-1-private-chat/integration/stack.json`;
    const nativeState = JSON.parse(await readFile(stateFile, 'utf8'));
    process.kill(nativeState.workerPid);
    nativeState.workerPid = null;
    await writeFile(stateFile, JSON.stringify(nativeState));
    const pendingResponse = await api(
      stack,
      a.cookie,
      '/media/uploads',
      'POST',
      {
        purpose: 'avatar',
        filename: 'pending.png',
        content_type: 'image/png',
        size_bytes: picture.length,
      },
    );
    const pendingGrant = await pendingResponse.json();
    expect(
      (
        await fetch(pendingGrant.url, {
          method: 'PUT',
          headers: pendingGrant.headers,
          body: new Blob([picture]),
        })
      ).ok,
    ).toBe(true);
    expect(
      (
        await api(
          stack,
          a.cookie,
          `/media/${pendingGrant.attachment.id}/complete`,
          'POST',
          { attempt_id: pendingGrant.attempt_id },
        )
      ).ok,
    ).toBe(true);
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
      'chat-test',
      '-EnvFile',
      stack.envFile,
      '-ComposeFile',
      'compose.yaml',
      '-DatabaseName',
      database,
      '-ProcessStateFile',
      `${root}/.superpowers/sdd/2026-10-12-phase-1-private-chat/integration/stack.json`,
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
        DATABASE_URL: settings.DATABASE_URL,
        BIND_ADDR: '127.0.0.1:19090',
        INTERNAL_BIND_ADDR: '127.0.0.1:19091',
        VALKEY_URL: 'redis://127.0.0.1:56381',
        MEDIA_S3_INTERNAL_ENDPOINT: 'http://127.0.0.1:8338',
        MEDIA_S3_PUBLIC_ENDPOINT: 'http://127.0.0.1:8338',
        CENTRIFUGO_API_URL: 'http://127.0.0.1:8002/api',
        CHAT_SHUTDOWN_FILE: `${backup}/runtime/maintenance.stop`,
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
    const records = (await history.json()).items;
    expect(records.map((message: { body: string }) => message.body)).toContain(
      '恢复演练：完整消息 🌱',
    );
    expect(
      records
        .find(
          (message: { attachments?: { id: string }[] }) =>
            message.attachments?.length,
        )
        ?.attachments.map((attachment: { id: string }) => attachment.id),
    ).toEqual([image.id, file.id]);
    const restoredApi = async (path: string) =>
      fetch(`http://127.0.0.1:19090/api/v1${path}`, {
        headers: { Cookie: cookie },
      });
    async function restoredBytes(url: string) {
      let bytes: Uint8Array<ArrayBuffer> | undefined;
      // S3 authentication can be ready before the restored master's volume routing.
      // Probe a known object with a deadline; persistent missing/corrupt data still fails.
      await eventually(async () => {
        const response = await fetch(url);
        if (!response.ok) return false;
        bytes = new Uint8Array(await response.arrayBuffer());
        return true;
      }, 20000);
      return bytes!;
    }
    for (const [id, variant, expectedBytes] of [
      [image.id, 'original', picture],
      [image.id, 'thumbnail', thumbnail],
      [file.id, 'original', fileBytes],
    ] as const) {
      const grant = await (
        await restoredApi(`/media/${id}/download?variant=${variant}`)
      ).json();
      expect(await restoredBytes(grant.url)).toEqual(expectedBytes);
    }
    const avatarGrant = await (
      await restoredApi(`/users/${a.user.id}/avatar`)
    ).json();
    expect(await restoredBytes(avatarGrant.url)).toEqual(thumbnail);
    await copyFile(
      `${root}/target/debug/worker.exe`,
      `${backup}/runtime/worker.exe`,
    );
    restoredWorker = spawn(`${backup}/runtime/worker.exe`, [], {
      cwd: root,
      windowsHide: true,
      stdio: 'ignore',
      env: {
        ...process.env,
        ...source,
        DATABASE_URL: settings.DATABASE_URL,
        VALKEY_URL: 'redis://127.0.0.1:56381',
        MEDIA_S3_INTERNAL_ENDPOINT: 'http://127.0.0.1:8338',
        MEDIA_S3_PUBLIC_ENDPOINT: 'http://127.0.0.1:8338',
        CENTRIFUGO_API_URL: 'http://127.0.0.1:8002/api',
        WORKER_METRICS_ADDR: '127.0.0.1:19092',
        CHAT_SHUTDOWN_FILE: `${backup}/runtime/maintenance.stop`,
      },
    });
    await eventually(
      async () =>
        (
          await (
            await restoredApi(`/media/${pendingGrant.attachment.id}`)
          ).json()
        ).state === 'ready',
      15000,
    );
  } finally {
    if (restored) {
      restored.kill();
    }
    restoredWorker?.kill();
    if (restoreEnv) {
      await command('docker', [
        'compose',
        '--project-name',
        project,
        '--env-file',
        restoreEnv,
        '--file',
        'compose.yaml',
        'logs',
        '--tail',
        '80',
        'seaweedfs',
      ]);
      await command('docker', [
        'compose',
        '--project-name',
        project,
        '--env-file',
        restoreEnv,
        '--file',
        'compose.yaml',
        'down',
        '--volumes',
      ]);
    }
    await stack.cleanup();
  }
}, 120000);
