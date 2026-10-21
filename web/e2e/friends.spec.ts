import type { Page } from '@playwright/test';

import { test, expect, signup, sessionCookie } from './fixtures';
import {
  account,
  api,
  befriend,
  eventually,
} from '../tests/integration/support';

test('public signup, consent, direct history, removal and reacceptance work across browsers', async ({
  page,
  browser,
  stack,
}) => {
  test.setTimeout(90000);
  const context = await browser.newContext();
  const bob = await context.newPage();
  try {
    const aliceAccount = await signup(page, stack);
    const bobAccount = await signup(bob, stack);
    await stack.startWorker();
    await page.getByRole('link', { name: '通讯录', exact: true }).click();
    await page
      .getByRole('complementary', { name: '通讯录列表' })
      .getByRole('button', { name: '添加好友', exact: true })
      .click();
    await page.getByLabel('用户名或昵称').fill(bobAccount.username);
    await page
      .getByRole('button')
      .filter({ hasText: `@${bobAccount.username}` })
      .click();
    await page
      .getByRole('button', { name: '发送好友申请', exact: true })
      .click();
    await expect(
      page.getByRole('dialog', { name: '添加好友' }).getByRole('status'),
    ).toContainText('好友申请已发送');
    expect(
      (await stack.db.query('SELECT count(*)::int AS count FROM conversations'))
        .rows[0].count,
    ).toBe(0);
    await page.getByRole('button', { name: '关闭', exact: true }).click();

    await expect(
      bob.getByRole('link', { name: '通讯录，1 条好友申请' }),
    ).toBeVisible();
    await bob.getByRole('link', { name: '通讯录，1 条好友申请' }).click();
    await bob
      .getByRole('button', { name: '新的朋友（1）', exact: true })
      .click();
    await bob
      .getByRole('list', { name: '新的朋友' })
      .getByRole('button')
      .click();
    await bob.getByRole('button', { name: '接受', exact: true }).click();
    await expect(
      bob.getByRole('button', { name: '接受', exact: true }),
    ).toHaveCount(0);
    await page.getByRole('link', { name: '通讯录', exact: true }).click();
    const friendRow = page
      .getByRole('list', { name: '好友' })
      .getByRole('listitem')
      .filter({ hasText: bobAccount.username });
    await expect(friendRow).toBeVisible();
    await friendRow.getByRole('button').click();
    await page.getByRole('button', { name: '发消息', exact: true }).click();
    await expect(page).toHaveURL(/\/app\/chats\//);
    const url = page.url();
    await page.getByRole('textbox', { name: '输入消息' }).fill('hello friend');
    await expect(page.getByRole('button', { name: '发送消息' })).toBeEnabled();
    await page.getByRole('button', { name: '发送消息' }).click();
    await expect(page.getByRole('log').getByText('hello friend')).toBeVisible();
    await page.getByRole('textbox', { name: '输入消息' }).fill('keep my draft');

    await bob.getByRole('button', { name: '好友', exact: true }).click();
    await bob.getByRole('list', { name: '好友' }).getByRole('button').click();
    await bob.getByRole('button', { name: '删除好友', exact: true }).click();
    await bob.getByRole('button', { name: '确认删除', exact: true }).click();
    await expect(page.getByRole('button', { name: '发送消息' })).toBeDisabled();
    await expect(page.getByRole('textbox', { name: '输入消息' })).toHaveValue(
      'keep my draft',
    );
    await expect(page.getByRole('log').getByText('hello friend')).toBeVisible();
    await expect(
      page.getByRole('button', { name: '添加好友', exact: true }),
    ).toBeVisible();
    await page.screenshot({
      path: test.info().outputPath('removed-friend-desktop.png'),
      animations: 'disabled',
    });
    await page.getByRole('button', { name: '添加好友', exact: true }).click();
    await page
      .getByRole('button', { name: '发送好友申请', exact: true })
      .click();
    await page.getByRole('button', { name: '关闭', exact: true }).click();
    await bob
      .getByRole('button', { name: '新的朋友（1）', exact: true })
      .click();
    await bob
      .getByRole('list', { name: '新的朋友' })
      .getByRole('button')
      .click();
    await bob.getByRole('button', { name: '接受', exact: true }).click();
    await expect(page.getByRole('button', { name: '发送消息' })).toBeEnabled();
    expect(page.url()).toBe(url);
    await bob.getByRole('button', { name: '好友', exact: true }).click();
    await expect(
      bob.getByText(`@${aliceAccount.username}`, { exact: true }),
    ).toBeVisible();
    await bob.setViewportSize({ width: 390, height: 844 });
    await expect(
      bob.getByRole('button', { name: '好友', exact: true }),
    ).toHaveAttribute('aria-pressed', 'true');
    for (const alert of await bob
      .getByRole('button', { name: '关闭提醒' })
      .all())
      await alert.click();
    await bob.screenshot({
      path: test.info().outputPath('contacts-mobile.png'),
      animations: 'disabled',
    });
    expect(
      await bob
        .locator('main')
        .evaluate((element) => element.scrollWidth <= element.clientWidth),
    ).toBe(true);
  } finally {
    await context.close();
  }
});

test('cancelled and declined requests can be sent again, and reverse search requires explicit acceptance', async ({
  page,
  browser,
  stack,
}) => {
  test.setTimeout(90000);
  const context = await browser.newContext();
  const bob = await context.newPage();

  async function findPerson(source: Page, username: string) {
    await source
      .getByRole('complementary', { name: '通讯录列表' })
      .getByRole('button', { name: '添加好友', exact: true })
      .click();
    const dialog = source.getByRole('dialog', { name: '添加好友' });
    await dialog.getByLabel('用户名或昵称').fill(username);
    await dialog
      .getByRole('button')
      .filter({ hasText: `@${username}` })
      .click();
    return dialog;
  }

  async function sendRequest(username: string) {
    const dialog = await findPerson(page, username);
    await dialog.getByRole('button', { name: '发送好友申请' }).click();
    await expect(dialog.getByRole('status')).toContainText('好友申请已发送');
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
  }

  try {
    const aliceAccount = await signup(page, stack);
    const bobAccount = await signup(bob, stack);
    await stack.startWorker();
    await page.getByRole('link', { name: '通讯录', exact: true }).click();
    await bob.getByRole('link', { name: '通讯录', exact: true }).click();

    await sendRequest(bobAccount.username);
    await page.getByRole('button', { name: /^新的朋友/ }).click();
    await page
      .getByRole('list', { name: '新的朋友' })
      .getByRole('button')
      .click();
    await page.getByRole('button', { name: '取消申请', exact: true }).click();
    await expect(page.getByRole('list', { name: '新的朋友' })).toContainText(
      '暂无好友申请',
    );
    await expect(
      bob.getByRole('button', { name: '新的朋友', exact: true }),
    ).toBeVisible();

    await sendRequest(bobAccount.username);
    await bob
      .getByRole('button', { name: '新的朋友（1）', exact: true })
      .click();
    await bob
      .getByRole('list', { name: '新的朋友' })
      .getByRole('button')
      .click();
    await bob.getByRole('button', { name: '拒绝', exact: true }).click();
    await expect(bob.getByRole('list', { name: '新的朋友' })).toContainText(
      '暂无好友申请',
    );
    await expect(page.getByRole('list', { name: '新的朋友' })).toContainText(
      '暂无好友申请',
    );

    await sendRequest(bobAccount.username);
    const incoming = await findPerson(bob, aliceAccount.username);
    await expect(incoming.getByText('对方已向你发送好友申请')).toBeVisible();
    await expect(
      incoming.getByRole('button', { name: '发送好友申请' }),
    ).toHaveCount(0);
    await incoming
      .getByRole('button', { name: '接受申请', exact: true })
      .click();
    await expect(incoming.getByRole('status')).toHaveText('已是好友');
    await incoming.getByRole('button', { name: '关闭', exact: true }).click();
    await page.getByRole('button', { name: '好友', exact: true }).click();
    await expect(page.getByRole('list', { name: '好友' })).toContainText(
      `@${bobAccount.username}`,
    );
    expect(
      (
        await stack.db.query(
          'SELECT status FROM friend_requests ORDER BY created_at',
        )
      ).rows.map((row: { status: string }) => row.status),
    ).toEqual(['cancelled', 'declined', 'accepted']);
    expect(
      (await stack.db.query('SELECT count(*)::int AS count FROM conversations'))
        .rows[0].count,
    ).toBe(0);
  } finally {
    await context.close();
  }
});

test('an ordinary member adds an accepted friend and the new member discovers only new history', async ({
  page,
  browser,
  stack,
}) => {
  test.setTimeout(90000);
  const context = await browser.newContext();
  const member = await context.newPage();
  try {
    await signup(page, stack);
    await signup(member, stack);
    const ownerCookie = await sessionCookie(page);
    const memberCookie = await sessionCookie(member);
    const memberUser = await (
      await api(stack, memberCookie, '/auth/me')
    ).json();
    const newcomer = await account(stack);
    await befriend(stack, ownerCookie, memberCookie);
    await befriend(stack, memberCookie, newcomer.cookie);
    await stack.startWorker();
    const response = await api(
      stack,
      ownerCookie,
      '/conversations/groups',
      'POST',
      { title: 'Friends project', member_ids: [memberUser.id] },
    );
    expect(response.status).toBe(201);
    const group = await response.json();
    await api(
      stack,
      ownerCookie,
      `/conversations/${group.id}/messages`,
      'POST',
      {
        client_message_id: crypto.randomUUID(),
        body: 'before joining',
        membership_id: group.membership.membership_id,
      },
    );
    await member.goto(`/app/chats/${group.id}`);
    await member.getByRole('button', { name: '群设置', exact: true }).click();
    await member.getByRole('button', { name: '添加成员', exact: true }).click();
    const dialog = member.getByRole('dialog', { name: '添加群成员' });
    await dialog
      .getByRole('combobox', { name: '选择好友' })
      .fill(newcomer.user.username);
    await dialog.getByRole('option').click();
    await dialog.getByRole('button', { name: '添加成员', exact: true }).click();
    await expect(dialog).not.toBeVisible();
    await expect(member.getByRole('list', { name: '群成员' })).toContainText(
      newcomer.user.username,
    );
    await eventually(async () => {
      const list = await (
        await api(stack, newcomer.cookie, '/conversations')
      ).json();
      return list.items.some((item: { id: string }) => item.id === group.id);
    });
    const history = await (
      await api(stack, newcomer.cookie, `/conversations/${group.id}/messages`)
    ).json();
    expect(history.items).toEqual([]);
  } finally {
    await context.close();
  }
});

test('leaving contacts aborts a pending open-chat action', async ({
  page,
  stack,
}) => {
  const peer = await account(stack);
  await signup(page, stack);
  await befriend(stack, await sessionCookie(page), peer.cookie);
  await stack.startWorker();
  await page.getByRole('link', { name: '通讯录', exact: true }).click();
  let started!: () => void;
  const pending = new Promise<void>((resolve) => (started = resolve));
  let release!: () => void;
  const released = new Promise<void>((resolve) => (release = resolve));
  let aborted = false;
  page.on('requestfailed', (request) => {
    if (request.url().endsWith('/conversations/direct')) aborted = true;
  });
  await page.route('**/api/v1/conversations/direct', async (route) => {
    started();
    await released;
    await route.continue().catch(() => {});
  });
  try {
    await page.getByRole('list', { name: '好友' }).getByRole('button').click();
    await page.getByRole('button', { name: '发消息', exact: true }).click();
    await pending;
    await page.getByRole('link', { name: '设置', exact: true }).click();
    await expect.poll(() => aborted).toBe(true);
    await expect(page).toHaveURL(/\/app\/settings$/);
  } finally {
    release();
  }
});
