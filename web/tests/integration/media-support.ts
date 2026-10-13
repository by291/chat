import { randomUUID } from 'node:crypto';

import { api, eventually, type IntegrationStack } from './support';
import type { components } from '../../src/lib/api-client';

export type Attachment = components['schemas']['AttachmentView'];

/** Exercise the public signed transfer and actual worker, without recording grant URLs. */
export async function readyUpload(
  stack: IntegrationStack,
  cookie: string,
  bytes: Uint8Array<ArrayBuffer>,
  input: {
    filename: string;
    content_type: string;
    purpose: 'message' | 'avatar';
    conversation_id?: string;
    membership_id?: string;
  },
): Promise<Attachment> {
  const response = await api(stack, cookie, '/media/uploads', 'POST', {
    ...input,
    size_bytes: bytes.length,
  });
  if (response.status !== 201)
    throw new Error(`Upload allocation failed: ${response.status}`);
  const grant: components['schemas']['UploadGrant'] = await response.json();
  const put = await fetch(grant.url, {
    method: 'PUT',
    headers: grant.headers,
    body: new Blob([bytes]),
  });
  if (!put.ok) throw new Error(`Signed PUT failed: ${put.status}`);
  const completion = await api(
    stack,
    cookie,
    `/media/${grant.attachment.id}/complete`,
    'POST',
    { attempt_id: grant.attempt_id },
  );
  if (!completion.ok)
    throw new Error(`Upload completion failed: ${completion.status}`);
  let result = (await completion.json()) as Attachment;
  await eventually(async () => {
    const status = await api(stack, cookie, `/media/${grant.attachment.id}`);
    if (!status.ok)
      throw new Error(`Attachment status failed: ${status.status}`);
    result = await status.json();
    if (result.state === 'failed' || result.state === 'expired')
      throw new Error('Real media processing failed');
    return result.state === 'ready';
  }, 15000);
  return result;
}

export async function sendAttachments(
  stack: IntegrationStack,
  cookie: string,
  conversation: string,
  attachmentIds: string[],
  membership?: string,
) {
  const response = await api(
    stack,
    cookie,
    `/conversations/${conversation}/messages`,
    'POST',
    {
      body: '',
      client_message_id: randomUUID(),
      attachment_ids: attachmentIds,
      membership_id: membership,
    },
  );
  if (response.status !== 201)
    throw new Error(`Attachment send failed: ${response.status}`);
  return response.json();
}
