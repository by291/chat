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

const uuid = '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}';
const conversationPath = new RegExp(`^/app/chats/${uuid}$`);
const eventTag = new RegExp(`^chat:${uuid}$`);
const generation = new RegExp(`^${uuid}$`);

/** Accept only the existing conversation route; arbitrary URLs and query tokens are rejected. */
export function notificationPath(value: unknown): string {
  return typeof value === 'string' && conversationPath.test(value)
    ? value
    : '/app';
}

/** Push announces unread work with generic visible text; it never loads or acknowledges records. */
export async function showPush(
  registration: NotificationRegistration,
  payload: unknown,
): Promise<void> {
  const candidate =
    payload && typeof payload === 'object'
      ? (payload as Record<string, unknown>)
      : {};
  const valid =
    candidate.schema_version === 1 &&
    typeof candidate.tag === 'string' &&
    eventTag.test(candidate.tag) &&
    typeof candidate.generation === 'string' &&
    generation.test(candidate.generation) &&
    notificationPath(candidate.path) !== '/app';

  await registration.showNotification('Chat', {
    body: '你有新消息',
    icon: '/icons/icon-192.png',
    badge: '/icons/icon-192.png',
    tag: valid ? (candidate.tag as string) : 'chat-new-message',
    data: { path: valid ? candidate.path : '/app' },
  });
}

/** Focus through SPA navigation so a notification click preserves existing drafts. */
export async function openNotification(
  clients: AppClients,
  payload: unknown,
  origin: string,
): Promise<void> {
  const path = notificationPath(
    payload && typeof payload === 'object'
      ? (payload as Record<string, unknown>).path
      : undefined,
  );
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
