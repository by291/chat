import { z } from 'zod';

type NotificationRegistration = {
  showNotification(title: string, options: NotificationOptions): Promise<void>;
};

type AppWindow = {
  url: string;
  focus(): Promise<unknown>;
  postMessage(message: unknown): void;
};

type AppClients = {
  matchAll(options: {
    type: 'window';
    includeUncontrolled: boolean;
  }): Promise<ReadonlyArray<AppWindow>>;
  openWindow(url: string): Promise<unknown>;
};

const UUID_PATTERN =
  '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}';
const conversationPathPattern = new RegExp(`^/app/chats/${UUID_PATTERN}$`);
const eventTagPattern = new RegExp(`^chat:${UUID_PATTERN}$`);
const generationPattern = new RegExp(`^${UUID_PATTERN}$`);

const notificationPathSchema = z.string().regex(conversationPathPattern);
const pushPayloadSchema = z.object({
  schema_version: z.literal(1),
  tag: z.string().regex(eventTagPattern),
  generation: z.string().regex(generationPattern),
  path: notificationPathSchema,
});
const notificationClickSchema = z.object({ path: notificationPathSchema });

/** Accept only the existing conversation route; arbitrary URLs and query tokens are rejected. */
export function notificationPath(value: unknown): string {
  const result = notificationPathSchema.safeParse(value);
  return result.success ? result.data : '/app';
}

/** Push announces unread work with generic visible text; it never loads or acknowledges records. */
export async function showPush(
  registration: NotificationRegistration,
  payload: unknown,
): Promise<void> {
  const result = pushPayloadSchema.safeParse(payload);

  await registration.showNotification('Chat', {
    body: '你有新消息',
    icon: '/icons/icon-192.png',
    badge: '/icons/icon-192.png',
    tag: result.success ? result.data.tag : 'chat-new-message',
    data: { path: result.success ? result.data.path : '/app' },
  });
}

/** Focus through SPA navigation so a notification click preserves existing drafts. */
export async function openNotification(
  clients: AppClients,
  payload: unknown,
  origin: string,
): Promise<void> {
  const result = notificationClickSchema.safeParse(payload);
  const path = result.success ? result.data.path : '/app';
  const windows = await clients.matchAll({
    type: 'window',
    includeUncontrolled: true,
  });
  const existing = windows.find((client) => {
    try {
      return new URL(client.url).origin === origin;
    } catch {
      return false;
    }
  });

  if (existing) {
    existing.postMessage({ type: 'open-notification', path });
    await existing.focus();
  } else {
    await clients.openWindow(new URL(path, origin).href);
  }
}
