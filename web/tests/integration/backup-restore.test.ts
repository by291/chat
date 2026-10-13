import { test, expect } from 'vitest';
import { readFile } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import {
  startIntegrationStack,
  account,
  api,
  eventually,
  startWorker,
} from './support';
import { readyUpload, sendAttachments } from './media-support';
import { restoreSnapshot, cleanupRestored } from './recovery';
import type { TestRuntime } from './runtime';

test('coordinated backup restores media bytes, avatars, queued uploads and revoked sessions into an isolated project', async () => {
  const stack = await startIntegrationStack();
  let restored: TestRuntime | undefined;
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
    await stack.stopWorker();
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
  } finally {
    try {
      if (restored) await cleanupRestored(restored);
    } finally {
      await stack.cleanup();
    }
  }
}, 120000);
