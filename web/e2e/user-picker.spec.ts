import { startChatWith } from './fixtures';
import { befriend } from '../tests/integration/support';
import { sessionCookie } from './fixtures';
import { test, expect, signup } from './fixtures';
import { account, api } from '../tests/integration/support';

test('friend selections create groups and contact profiles reuse direct chats', async ({
  page,
  stack,
}) => {
  const first = await account(stack);
  const second = await account(stack);
  await stack.db.query('UPDATE users SET display_name = $1 WHERE id = $2', [
    '林晓',
    first.user.id,
  ]);
  await stack.db.query('UPDATE users SET display_name = $1 WHERE id = $2', [
    '小林',
    second.user.id,
  ]);
  await signup(page, stack);
  await befriend(stack, await sessionCookie(page), first.cookie);
  await befriend(stack, await sessionCookie(page), second.cookie);
  await page.getByRole('button', { name: '群聊操作', exact: true }).click();
  await page.getByRole('menuitem', { name: '创建群聊', exact: true }).click();
  const search = page.getByRole('combobox', { name: '选择好友' });
  const add = page.getByRole('button', { name: '创建群聊', exact: true });
  await expect(add).toBeDisabled();

  await search.fill('林');
  await expect(page.getByRole('option')).toHaveCount(2);
  await page.screenshot({
    path: test.info().outputPath('user-picker-results.png'),
  });
  await search.press('ArrowDown');
  await search.press('Enter');
  await expect(page.locator('.selected-user')).toHaveCount(1);
  await expect(search).toHaveValue('');
  await expect(page.getByRole('dialog')).toBeVisible();
  expect(
    (await stack.db.query('SELECT count(*)::int AS count FROM conversations'))
      .rows[0].count,
  ).toBe(0);
  await page.locator('.selected-user button').click();
  await expect(page.locator('.selected-user')).toHaveCount(0);

  for (const user of [first.user, second.user]) {
    await search.fill(user.username.slice(1));
    await page
      .getByRole('option')
      .filter({ hasText: '@' + user.username })
      .click();
  }
  await expect(page.locator('.selected-user')).toHaveCount(2);
  await expect(search).toHaveValue('');
  for (const width of [1280, 390, 320]) {
    await page.setViewportSize({ width, height: 844 });
    const dialog = page.getByRole('dialog');
    expect(
      await dialog.evaluate(
        (element) => element.scrollWidth <= element.clientWidth,
      ),
    ).toBe(true);
    await expect(add).toBeInViewport();
    await page.screenshot({
      path: test.info().outputPath(`user-picker-${width}.png`),
    });
  }
  await page.evaluate(() =>
    document.documentElement.setAttribute('data-theme', 'dark'),
  );
  await page.screenshot({
    path: test.info().outputPath('user-picker-dark.png'),
  });
  await page.evaluate(() =>
    document.documentElement.removeAttribute('data-theme'),
  );
  await add.click();
  await expect(page.getByRole('dialog')).not.toBeVisible();
  await expect(page).toHaveURL(/\/app\/chats\//);
  const groupId = page.url().split('/').pop();
  const group = await stack.db.query(
    'SELECT kind, title FROM conversations WHERE id = $1',
    [groupId],
  );
  expect(group.rows[0]).toEqual({ kind: 'group', title: '林晓、小林' });
  const members = await stack.db.query(
    'SELECT user_id FROM conversation_members WHERE conversation_id = $1',
    [groupId],
  );
  expect(members.rows).toHaveLength(3);
  expect(members.rows.map((row) => row.user_id)).toEqual(
    expect.arrayContaining([first.user.id, second.user.id]),
  );
  for (const peer of [first, second]) {
    const response = await api(stack, peer.cookie, `/conversations/${groupId}`);
    expect(response.status).toBe(200);
    const summary = await response.json();
    expect(summary.my_role).toBe('member');
    expect(summary.member_count).toBe(3);
    const sent = await api(
      stack,
      peer.cookie,
      `/conversations/${groupId}/messages`,
      'POST',
      {
        client_message_id: crypto.randomUUID(),
        body: 'Hello from an initial member',
        membership_id: summary.membership.membership_id,
      },
    );
    expect(sent.status).toBe(201);
  }
  await page.setViewportSize({ width: 1280, height: 844 });
  await expect(
    page.getByRole('navigation', { name: '会话列表' }).getByRole('link'),
  ).toHaveCount(1);

  // Contact profiles open direct chats; repeating the action reuses the same pair.
  for (let attempt = 0; attempt < 2; attempt++) {
    await startChatWith(page, first.user.username);
    await expect(page).toHaveURL(/\/app\/chats\//);
    expect(
      (await stack.db.query('SELECT count(*)::int AS count FROM conversations'))
        .rows[0].count,
    ).toBe(2);
  }
});

test('late search results cannot replace a new query and failed group creation keeps selections', async ({
  page,
  stack,
}) => {
  const first = await account(stack);
  const second = await account(stack);
  await signup(page, stack);
  await befriend(stack, await sessionCookie(page), first.cookie);
  await befriend(stack, await sessionCookie(page), second.cookie);
  let releaseOld!: () => void;
  const oldResponse = new Promise<void>((resolve) => {
    releaseOld = resolve;
  });
  let oldStarted!: () => void;
  const started = new Promise<void>((resolve) => {
    oldStarted = resolve;
  });
  await page.route('**/api/v1/friends?*', async (route) => {
    if (new URL(route.request().url()).searchParams.get('q') === 'old') {
      oldStarted();
      await oldResponse;
      await route.fulfill({
        json: {
          items: [{ user: first.user, created_at: new Date().toISOString() }],
          next_cursor: null,
        },
      });
    } else await route.continue();
  });
  await page.getByRole('button', { name: '群聊操作', exact: true }).click();
  await page.getByRole('menuitem', { name: '创建群聊', exact: true }).click();
  const search = page.getByRole('combobox', { name: '选择好友' });
  await search.fill('old');
  await started;
  await search.fill(second.user.username);
  await expect(page.getByRole('option')).toContainText(second.user.username);
  releaseOld();
  await page.getByRole('option').click();
  await search.fill(first.user.username);
  await page.getByRole('option').click();

  await page.getByLabel('群名称（选填）').fill('周末计划');
  const requests: string[][] = [];
  let reject = true;
  await page.route('**/api/v1/conversations/groups', async (route) => {
    requests.push(route.request().postDataJSON().member_ids as string[]);
    if (reject) {
      reject = false;
      await route.fulfill({
        status: 503,
        json: { code: 'unavailable', message: '暂时无法创建' },
      });
    } else await route.continue();
  });
  const add = page.getByRole('button', { name: '创建群聊', exact: true });
  await add.click();
  await expect(page.getByRole('alert')).toContainText('暂时无法创建');
  await expect(page.locator('.selected-user')).toHaveCount(2);
  expect(
    (await stack.db.query('SELECT count(*)::int AS count FROM conversations'))
      .rows[0].count,
  ).toBe(0);
  await add.click();
  await expect(page.getByRole('dialog')).not.toBeVisible();
  await expect(
    page.getByRole('heading', { name: '周末计划', exact: true }),
  ).toBeVisible();
  expect(requests).toEqual([
    [second.user.id, first.user.id],
    [second.user.id, first.user.id],
  ]);
});
