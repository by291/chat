import { befriend } from './support';
import { test, expect } from 'vitest';
import { readFile } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { account, api, eventually, startWorker } from './support';
import { readyUpload, sendAttachments } from './media-support';
import { restoreSnapshot, cleanupRestored } from './recovery';
import { TestRuntime } from './runtime';
import { TestSmtpServer } from './mail-support';

test('coordinated backup restores media bytes, avatars, queued uploads and revoked sessions into an isolated project', async () => {
  const mail = new TestSmtpServer();
  await mail.start();
  let stack: TestRuntime;
  try {
    stack = await TestRuntime.start({ mail: mail.environment() });
  } catch (error) {
    await mail.stop();
    throw error;
  }
  let restored: TestRuntime | undefined;
  try {
    const a = await account(stack),
      b = await account(stack);
    await befriend(stack, a.cookie, b.cookie);
    const conversation = await (
      await api(stack, a.cookie, '/conversations/direct', 'POST', {
        peer_id: b.user.id,
      })
    ).json();
    await startWorker(stack);
    expect(
      (
        await api(stack, a.cookie, '/auth/email', 'POST', {
          email: 'backup-owner@example.test',
          current_password: 'correct horse battery',
        })
      ).status,
    ).toBe(202);
    await eventually(async () => mail.messages().length === 1);
    const verificationToken = new URL(mail.messages()[0].link!).hash.slice(
      '#token='.length,
    );
    expect(
      (
        await api(stack, a.cookie, '/auth/email/verify', 'POST', {
          token: verificationToken,
        })
      ).status,
    ).toBe(200);
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
    await stack.stopWorker();
    expect(
      (
        await api(stack, b.cookie, '/auth/email', 'POST', {
          email: 'backup-peer@example.test',
          current_password: 'correct horse battery',
        })
      ).status,
    ).toBe(202);
    expect(
      (
        await api(stack, '', '/auth/password/recovery', 'POST', {
          email: 'backup-owner@example.test',
        })
      ).status,
    ).toBe(202);
    const pendingMail = (
      await stack.db.query(
        "SELECT id, encrypted_payload FROM account_mail_jobs WHERE status='pending' ORDER BY id",
      )
    ).rows;
    expect(pendingMail).toHaveLength(2);
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
    restored = await restoreSnapshot(stack);
    expect(restored.env.MAIL_QUEUE_ENCRYPTION_KEY).toBe(
      stack.env.MAIL_QUEUE_ENCRYPTION_KEY,
    );
    expect(
      (
        await restored.db.query(
          "SELECT id, encrypted_payload FROM account_mail_jobs WHERE status='pending' ORDER BY id",
        )
      ).rows,
    ).toEqual(pendingMail);
    expect(
      (
        await fetch(`${restored.apiUrl}/api/v1/auth/me`, {
          headers: { Cookie: a.cookie },
        })
      ).status,
    ).toBe(401);
    const login = await fetch(`${restored.apiUrl}/api/v1/auth/login`, {
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
      `${restored.apiUrl}/api/v1/conversations/${conversation.id}/messages`,
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
      fetch(`${restored!.apiUrl}/api/v1${path}`, {
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
    await restored.startWorker();
    await eventually(
      async () =>
        (
          await (
            await restoredApi(`/media/${pendingGrant.attachment.id}`)
          ).json()
        ).state === 'ready',
      15000,
    );
    await eventually(async () => mail.messages().length === 3);
    const restoredVerification = mail
      .messages()
      .find((message) => message.recipient === 'backup-peer@example.test')!;
    expect(
      (
        await api(restored, b.cookie, '/auth/email/verify', 'POST', {
          token: new URL(restoredVerification.link!).hash.slice(
            '#token='.length,
          ),
        })
      ).status,
    ).toBe(200);
    const resetMail = mail
      .messages()
      .find((message) => message.link?.includes('/reset-password#'))!;
    expect(
      (
        await api(restored, '', '/auth/password/reset', 'POST', {
          token: new URL(resetMail.link!).hash.slice('#token='.length),
          new_password: 'restored correct password',
        })
      ).status,
    ).toBe(204);
    expect((await api(restored, cookie, '/auth/me')).status).toBe(401);
    expect(
      (
        await api(restored, '', '/auth/login', 'POST', {
          username: a.user.username,
          password: 'restored correct password',
          device_label: 'Recovered account',
        })
      ).status,
    ).toBe(200);
  } finally {
    try {
      if (restored) await cleanupRestored(restored);
    } finally {
      try {
        await stack.cleanup();
      } finally {
        await mail.stop();
      }
    }
  }
}, 120000);
