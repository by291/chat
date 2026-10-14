import { randomUUID } from 'node:crypto';

import type { Page } from '@playwright/test';

import { test, expect, signup } from './fixtures';
import { api, startWorker } from '../tests/integration/support';
import type {
  Conversation,
  Group,
  Message,
} from '../src/lib/features/chat/models';

test.setTimeout(90000);

async function cookie(page: Page) {
  const session = (await page.context().cookies()).find(
    (entry) => entry.name === 'chat_session',
  );
  if (!session) throw new Error('Expected authenticated browser');
  return `${session.name}=${session.value}`;
}

async function json<T>(response: Response): Promise<T> {
  if (!response.ok)
    throw new Error(
      `Notification fixture request failed: ${response.status} ${await response.text()}`,
    );
  return response.json() as Promise<T>;
}

for (const kind of ['direct', 'group'] as const) {
  test(`first ${kind} message reaches a second logged-in device while new metadata is delayed`, async ({
    page,
    browser,
    stack,
  }) => {
    const recipientContext = await browser.newContext();
    const secondContext = await browser.newContext();
    const recipient = await recipientContext.newPage();
    const second = await secondContext.newPage();
    let release = () => {};
    try {
      await signup(page, stack);
      const recipientAccount = await signup(recipient, stack);
      await startWorker(stack);
      const publications: string[] = [];
      second.on('websocket', (socket) =>
        socket.on('framereceived', (frame) => {
          publications.push(String(frame.payload));
          if (publications.length > 100) publications.shift();
        }),
      );
      await second.addInitScript(() => {
        const schedule = window.setInterval.bind(window);
        window.setInterval = ((
          handler: TimerHandler,
          delay?: number,
          ...args: unknown[]
        ) =>
          delay === 60000
            ? 0
            : schedule(handler, delay, ...args)) as typeof window.setInterval;
      });
      await second.goto('/login');
      await second
        .getByLabel('用户名', { exact: true })
        .fill(recipientAccount.username);
      await second
        .getByLabel('密码', { exact: true })
        .fill(recipientAccount.password);
      const initialList = second.waitForResponse(
        (response) =>
          new URL(response.url()).pathname === '/api/v1/conversations' &&
          response.status() === 200,
      );
      await second.getByRole('button', { name: '登录', exact: true }).click();
      await initialList;
      await expect(second.getByText('已连接', { exact: true })).toBeVisible();
      await second.getByRole('link', { name: '设置', exact: true }).click();
      await expect(
        second.getByRole('heading', { name: '个人设置', exact: true }),
      ).toBeVisible();

      const gate = new Promise<void>((resolve) => {
        release = resolve;
      });
      await second.route('**/api/v1/conversations?*', async (route) => {
        // Hold before fetching so the eventual authorized summary includes the
        // first message. Its live publication must remain a replay candidate.
        await gate;
        const response = await route.fetch();
        await route.fulfill({ response });
      });
      const recipientCookie = await cookie(recipient);
      const senderCookie = await cookie(page);
      let conversation: Conversation;
      let membershipId: string | undefined;
      if (kind === 'direct') {
        const user = await json<{ id: string }>(
          await api(stack, recipientCookie, '/auth/me'),
        );
        conversation = await json<Conversation>(
          await api(stack, senderCookie, '/conversations/direct', 'POST', {
            peer_id: user.id,
          }),
        );
      } else {
        const group = await json<Group>(
          await api(stack, recipientCookie, '/conversations/groups', 'POST', {
            title: '另一个设备新建的群聊',
          }),
        );
        const invitation = await json<{ token: string }>(
          await api(
            stack,
            recipientCookie,
            `/conversations/${group.id}/invitations`,
            'POST',
            { membership_id: group.membership.membership_id },
          ),
        );
        const joined = await json<Group>(
          await api(stack, senderCookie, '/group-invitations/accept', 'POST', {
            token: invitation.token,
          }),
        );
        conversation = group;
        membershipId = joined.membership.membership_id;
      }
      const first = await json<Message>(
        await api(
          stack,
          senderCookie,
          `/conversations/${conversation.id}/messages`,
          'POST',
          {
            client_message_id: randomUUID(),
            body: '元数据尚未加载的第一条消息',
            membership_id: membershipId,
          },
        ),
      );
      await expect
        .poll(() =>
          publications.some(
            (publication) =>
              publication.includes('message.created') &&
              publication.includes(conversation.id) &&
              publication.includes(first.created_seq),
          ),
        )
        .toBe(true);
      await expect(second.locator('.in-app-alert')).toHaveCount(0);
      release();

      await expect(second.locator('.in-app-alert')).toHaveCount(1);
      const beforeOpen = await json<Conversation>(
        await api(
          stack,
          await cookie(second),
          `/conversations/${conversation.id}`,
        ),
      );
      expect(beforeOpen.my_read_seq).toBe('0');
      await second.locator('.alert-open').click();
      await expect(second).toHaveURL(
        new RegExp(`/app/chats/${conversation.id}$`),
      );
      await expect(
        second.locator(`[data-message-id="${first.id}"] .message-body`),
      ).toHaveText(first.body);
      await expect(second.locator('.sync-error')).toHaveCount(0);
      await expect(second.locator('.in-app-alert')).toHaveCount(0);
    } finally {
      release();
      await second.unrouteAll({ behavior: 'wait' });
      await secondContext.close();
      await recipientContext.close();
    }
  });
}
