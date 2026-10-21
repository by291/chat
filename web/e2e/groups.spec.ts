import type { Page } from '@playwright/test';

import { test, expect, signup } from './fixtures';
import { api, startWorker } from '../tests/integration/support';
import type { IntegrationStack } from '../tests/integration/support';

async function cookie(page: Page) {
  const session = (await page.context().cookies()).find(
    (entry) => entry.name === 'chat_session',
  );
  if (!session) throw new Error('Expected authenticated browser');
  return `${session.name}=${session.value}`;
}

async function invite(page: Page) {
  await page.getByRole('button', { name: '群设置', exact: true }).click();
  await page.getByRole('button', { name: '生成邀请链接', exact: true }).click();
  const link = await page
    .getByLabel('邀请链接（单次使用，七天有效）')
    .inputValue();
  await page.getByRole('button', { name: '关闭', exact: true }).click();
  return link;
}

async function join(page: Page, link: string) {
  await page.goto(link);
  await page.getByRole('button', { name: '确认加入', exact: true }).click();
  await expect(page).toHaveURL(/\/app\/chats\//);
}

async function send(page: Page, body: string) {
  const composer = page.getByRole('textbox', { name: '输入消息' });
  await composer.fill(body);
  await composer.press('Enter');
  await expect(
    page.getByRole('log').getByText(body, { exact: true }),
  ).toBeVisible();
}

async function summary(stack: IntegrationStack, page: Page, id: string) {
  const response = await api(stack, await cookie(page), `/conversations/${id}`);
  if (!response.ok) throw new Error(`Group summary failed: ${response.status}`);
  return response.json();
}

test('three browsers manage a private group and rejoining hides the old episode', async ({
  page,
  browser,
  stack,
}) => {
  test.setTimeout(60000);
  const memberContext = await browser.newContext();
  const guestContext = await browser.newContext();
  const member = await memberContext.newPage();
  const guest = await guestContext.newPage();

  try {
    await signup(page, stack);
    const second = await signup(member, stack);
    const third = await signup(guest, stack);
    await startWorker(stack);
    await expect(page.locator('.chat-shell')).toHaveAttribute(
      'data-connection',
      'online',
    );

    // This history-isolation scenario needs an initially owner-only fixture.
    // The group picker separately covers creating populated groups.
    const created = await api(
      stack,
      await cookie(page),
      '/conversations/groups',
      'POST',
      { title: '三人项目组' },
    );
    expect(created.status).toBe(201);
    const { id } = await created.json();
    await page.goto(`/app/chats/${id}`);
    await send(page, '加入之前的私人消息');

    const firstInvite = new URL(await invite(page)).searchParams.get('invite')!;
    await member.getByRole('button', { name: '群聊操作', exact: true }).click();
    await member
      .getByRole('menuitem', { name: '加入群聊', exact: true })
      .click();
    const joinDialog = member.getByRole('dialog');
    await expect(
      joinDialog.getByRole('button', { name: '加入群聊', exact: true }),
    ).toBeDisabled();
    await joinDialog
      .getByLabel('邀请链接或口令', { exact: true })
      .fill(firstInvite);
    await joinDialog
      .getByRole('button', { name: '加入群聊', exact: true })
      .click();
    await expect(joinDialog).not.toBeVisible();
    await expect(member).toHaveURL(new RegExp(`/app/chats/${id}$`));
    const secondInvite = await invite(page);
    await guest.getByRole('button', { name: '群聊操作', exact: true }).click();
    await guest
      .getByRole('menuitem', { name: '加入群聊', exact: true })
      .click();
    const guestDialog = guest.getByRole('dialog');
    await guestDialog
      .getByLabel('邀请链接或口令', { exact: true })
      .fill(secondInvite);
    await guestDialog
      .getByRole('button', { name: '加入群聊', exact: true })
      .click();
    await expect(guestDialog).not.toBeVisible();
    await expect(guest).toHaveURL(new RegExp(`/app/chats/${id}$`));
    await expect(
      member.getByRole('log').getByText('加入之前的私人消息'),
    ).toHaveCount(0);
    await expect(
      guest.getByRole('log').getByText('加入之前的私人消息'),
    ).toHaveCount(0);
    await send(page, '三人共同可见的消息');
    await expect(
      member.getByRole('log').getByText('三人共同可见的消息', { exact: true }),
    ).toBeVisible();
    await expect(
      guest.getByRole('log').getByText('三人共同可见的消息', { exact: true }),
    ).toBeVisible();
    await expect(
      member.getByRole('log').locator('.message-sender').last(),
    ).toHaveText('测试用户 🌟');
    await member.bringToFront();
    await expect
      .poll(async () => (await summary(stack, member, id)).my_read_seq)
      .not.toBe('0');

    await page.getByRole('button', { name: '群设置' }).click();
    await page.getByLabel('群名称', { exact: true }).fill('新的群名称');
    await page.getByRole('button', { name: '保存群名称' }).click();
    const secondRow = page
      .getByRole('list', { name: '群成员' })
      .getByRole('listitem')
      .filter({ hasText: second.username });
    await secondRow
      .getByRole('button', { name: '设为管理员', exact: true })
      .click();
    await expect(secondRow).toContainText('管理员');
    await page.getByRole('button', { name: '关闭', exact: true }).click();
    await expect(
      member
        .locator('.chat-heading')
        .getByRole('heading', { name: '新的群名称', exact: true }),
    ).toBeVisible();

    const oldEpisode = (await summary(stack, guest, id)).membership
      .membership_id;
    await member.getByRole('button', { name: '群设置' }).click();
    const thirdRow = member
      .getByRole('list', { name: '群成员' })
      .getByRole('listitem')
      .filter({ hasText: third.username });
    await thirdRow.getByRole('button', { name: '移除', exact: true }).click();
    await expect(thirdRow).toHaveCount(0);
    await member.getByRole('button', { name: '关闭', exact: true }).click();
    await expect(guest.getByRole('textbox', { name: '输入消息' })).toHaveCount(
      0,
    );
    await expect(guest.getByRole('log')).toHaveCount(0);

    await send(page, '移除期间的私人消息');
    await join(guest, await invite(page));
    const newEpisode = (await summary(stack, guest, id)).membership
      .membership_id;
    expect(newEpisode).not.toBe(oldEpisode);
    await expect(
      guest.getByRole('log').getByText('三人共同可见的消息'),
    ).toHaveCount(0);
    await expect(
      guest.getByRole('log').getByText('移除期间的私人消息'),
    ).toHaveCount(0);
    const staleRead = await api(
      stack,
      await cookie(guest),
      `/conversations/${id}/read`,
      'POST',
      { message_seq: '1', membership_id: oldEpisode },
    );
    expect(staleRead.status).toBe(409);

    await guest.reload();
    await expect(guest.locator('.chat-shell')).toHaveAttribute(
      'data-connection',
      'online',
    );
    await send(page, '重新加入后的消息');
    await expect(
      guest.getByRole('log').getByText('重新加入后的消息', { exact: true }),
    ).toBeVisible();
    await guest.bringToFront();
    await expect
      .poll(async () => {
        const current = await summary(stack, guest, id);
        return (
          BigInt(current.my_read_seq) >=
          BigInt(current.last_message.created_seq)
        );
      })
      .toBe(true);
    await page.getByRole('button', { name: '群设置' }).click();
    await expect(page.getByLabel('消息已读成员')).toContainText('测试用户');
    await page
      .getByRole('list', { name: '群成员' })
      .getByRole('listitem')
      .filter({ hasText: second.username })
      .getByRole('button', { name: '转让群主', exact: true })
      .click();
    await page.getByRole('button', { name: '退出群聊', exact: true }).click();
    await expect(page).toHaveURL(/\/app$/);
    await expect
      .poll(async () => (await summary(stack, member, id)).my_role)
      .toBe('owner');

    await guest.setViewportSize({ width: 390, height: 844 });
    const route = guest.url();
    await guest.getByRole('textbox', { name: '输入消息' }).fill('移动端草稿');
    await guest.getByRole('button', { name: '返回会话列表' }).click();
    await guest
      .getByRole('navigation', { name: '会话列表' })
      .getByRole('link')
      .filter({ hasText: '新的群名称' })
      .click();
    await expect(guest).toHaveURL(route);
    await expect(guest.getByRole('textbox', { name: '输入消息' })).toHaveValue(
      '移动端草稿',
    );
    await guest
      .getByRole('textbox', { name: '输入消息' })
      .dispatchEvent('compositionstart');
    await guest
      .getByRole('textbox', { name: '输入消息' })
      .dispatchEvent('keydown', {
        key: 'Enter',
        code: 'Enter',
        isComposing: true,
      });
    await expect(guest.getByRole('textbox', { name: '输入消息' })).toHaveValue(
      '移动端草稿',
    );
    await guest
      .getByRole('textbox', { name: '输入消息' })
      .dispatchEvent('compositionend');
    await guest.screenshot({
      path: test.info().outputPath('group-mobile.png'),
    });
  } finally {
    await memberContext.close();
    await guestContext.close();
  }
});
