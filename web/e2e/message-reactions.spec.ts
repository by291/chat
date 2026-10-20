import { befriend } from '../tests/integration/support';
import { test, expect, signup } from './fixtures';
import { account, api } from '../tests/integration/support';

test('a reaction changes neither the edited label nor unread order and converges across devices', async ({
  page,
  context,
  browser,
  stack,
}) => {
  test.setTimeout(180000);
  const credentials = await signup(page, stack);
  await stack.startWorker();
  const cookie = (await context.cookies()).find(
    (cookie) => cookie.name === 'chat_session',
  )!;
  const authorCookie = `${cookie.name}=${cookie.value}`;
  const author = await (await api(stack, authorCookie, '/auth/me')).json();
  const peer = await account(stack);
  await befriend(stack, authorCookie, peer.cookie);
  const conversation = await (
    await api(stack, authorCookie, '/conversations/direct', 'POST', {
      peer_id: peer.user.id,
    })
  ).json();
  const sent = await (
    await api(
      stack,
      authorCookie,
      `/conversations/${conversation.id}/messages`,
      'POST',
      { client_message_id: crypto.randomUUID(), body: '回应保留原消息' },
    )
  ).json();
  await api(
    stack,
    authorCookie,
    `/conversations/${conversation.id}/messages/${sent.id}/reaction`,
    'PUT',
    { kind: 'like' },
  );
  await page.goto(`/app/chats/${conversation.id}`);
  const bubble = page
    .getByRole('log', { name: '聊天消息' })
    .locator(`[data-message-id="${sent.id}"]`);
  await expect(bubble.getByText(sent.body, { exact: true })).toBeVisible();

  await expect(bubble.getByText('已编辑', { exact: true })).toHaveCount(0);

  await expect(
    bubble.getByRole('button', { name: '点赞，1 个回应' }),
  ).toHaveAttribute('aria-pressed', 'true');
  const second = await browser.newContext({ baseURL: stack.origin });
  await second.addCookies([
    {
      name: 'chat_session',
      value: peer.cookie.slice('chat_session='.length),
      domain: 'localhost',
      path: '/',
    },
  ]);
  const peerPage = await second.newPage();
  let device: import('@playwright/test').BrowserContext | undefined;
  try {
    await peerPage.goto(`/app/chats/${conversation.id}`);
    const peerBubble = peerPage
      .getByRole('log', { name: '聊天消息' })
      .locator(`[data-message-id="${sent.id}"]`);
    await expect(
      peerBubble.getByRole('button', { name: '点赞，1 个回应' }),
    ).toHaveAttribute('aria-pressed', 'false');
    await peerBubble.getByRole('button', { name: '添加表情回应' }).click();
    await peerBubble.getByRole('button', { name: '喜欢', exact: true }).click();
    await expect(
      bubble.getByRole('button', { name: '喜欢，1 个回应' }),
    ).toHaveAttribute('aria-pressed', 'false');
    await expect(
      peerBubble.getByRole('button', { name: '喜欢，1 个回应' }),
    ).toHaveAttribute('aria-pressed', 'true');
    await peerBubble.getByRole('button', { name: '喜欢，1 个回应' }).click();
    await expect(
      bubble.getByRole('button', { name: '喜欢，1 个回应' }),
    ).toHaveCount(0);
    await peerPage.reload();
    await expect(
      peerBubble.getByRole('button', { name: '点赞，1 个回应' }),
    ).toHaveAttribute('aria-pressed', 'false');
    await peerPage.setViewportSize({ width: 390, height: 844 });
    await peerBubble.getByRole('button', { name: '添加表情回应' }).focus();
    await peerPage.keyboard.press('Enter');
    await peerBubble.getByRole('button', { name: '庆祝', exact: true }).focus();
    await peerPage.keyboard.press('Enter');
    await expect(
      bubble.getByRole('button', { name: '庆祝，1 个回应' }),
    ).toHaveCount(1);
    expect(
      await peerPage.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
    const before = await stack.db.query(
      'SELECT last_read_message_seq FROM conversation_members ORDER BY user_id',
    );
    await peerBubble.getByRole('button', { name: '庆祝，1 个回应' }).click();
    await expect(
      bubble.getByRole('button', { name: '庆祝，1 个回应' }),
    ).toHaveCount(0);
    expect(
      (
        await stack.db.query(
          'SELECT last_read_message_seq FROM conversation_members ORDER BY user_id',
        )
      ).rows,
    ).toEqual(before.rows);
    expect(
      (await stack.db.query('SELECT count(*)::int AS count FROM push_jobs'))
        .rows[0].count,
    ).toBe(0);
    const login = await api(stack, '', '/auth/login', 'POST', {
      username: credentials.username,
      password: credentials.password,
      device_label: 'Reaction second device',
    });
    expect(login.status).toBe(200);
    const deviceCookie = login.headers.get('set-cookie')!.split(';')[0];
    device = await browser.newContext({ baseURL: stack.origin });
    await device.addCookies([
      {
        name: 'chat_session',
        value: deviceCookie.slice('chat_session='.length),
        domain: 'localhost',
        path: '/',
      },
    ]);
    const devicePage = await device.newPage();
    await devicePage.goto(`/app/chats/${conversation.id}`);
    const deviceBubble = devicePage
      .getByRole('log', { name: '聊天消息' })
      .locator(`[data-message-id="${sent.id}"]`);
    await expect(
      deviceBubble.getByRole('button', { name: '点赞，1 个回应' }),
    ).toHaveAttribute('aria-pressed', 'true');
    await device.setOffline(true);
    await peerBubble.getByRole('button', { name: '添加表情回应' }).click();
    await peerBubble.getByRole('button', { name: '开心', exact: true }).click();
    await expect(
      bubble.getByRole('button', { name: '开心，1 个回应' }),
    ).toBeVisible();
    await device.setOffline(false);
    await expect(
      deviceBubble.getByRole('button', { name: '开心，1 个回应' }),
    ).toHaveAttribute('aria-pressed', 'false');
    await devicePage.reload();
    await expect(
      deviceBubble.getByRole('button', { name: '点赞，1 个回应' }),
    ).toHaveAttribute('aria-pressed', 'true');
    let release!: () => void;
    let accepted!: () => void;
    const held = new Promise<void>((resolve) => (release = resolve));
    const committed = new Promise<void>((resolve) => (accepted = resolve));
    const reactionUrl = `**/api/v1/conversations/${conversation.id}/messages/${sent.id}/reaction`;
    await page.route(reactionUrl, async (route) => {
      const response = await route.fetch();
      accepted();
      await held;
      await route.fulfill({ response });
    });
    try {
      await bubble.getByRole('button', { name: '添加表情回应' }).click();
      await bubble.getByRole('button', { name: '难过', exact: true }).click();
      await committed;
      await deviceBubble.getByRole('button', { name: '添加表情回应' }).click();
      await deviceBubble
        .getByRole('button', { name: '喜欢', exact: true })
        .click();
      await expect(
        bubble.getByRole('button', { name: '喜欢，1 个回应' }),
      ).toHaveAttribute('aria-pressed', 'true');
      release();
      await expect(
        bubble.getByRole('button', { name: '添加表情回应' }),
      ).toBeEnabled();
      await expect(
        bubble.getByRole('button', { name: '喜欢，1 个回应' }),
      ).toHaveAttribute('aria-pressed', 'true');
      await expect(
        bubble.getByRole('button', { name: '难过，1 个回应' }),
      ).toHaveCount(0);
    } finally {
      release();
      await page.unroute(reactionUrl);
    }
    await bubble.getByRole('button', { name: '撤回', exact: true }).click();
    await page.getByRole('button', { name: '确认撤回', exact: true }).click();
    await expect(
      peerBubble.getByText('此消息已撤回', { exact: true }),
    ).toBeVisible();
    await expect(
      peerBubble.getByRole('button', { name: '添加表情回应' }),
    ).toHaveCount(0);
  } finally {
    await device?.close();
    await second.close();
  }
});

test('group removal and rejoin discard old reaction controls while retaining aggregate counts for remaining members', async ({
  page,
  context,
  browser,
  stack,
}) => {
  test.setTimeout(120000);
  await signup(page, stack);
  await stack.startWorker();
  const cookie = (await context.cookies()).find(
    (cookie) => cookie.name === 'chat_session',
  )!;
  const ownerCookie = `${cookie.name}=${cookie.value}`;
  const peer = await account(stack);
  const group = await (
    await api(stack, ownerCookie, '/conversations/groups', 'POST', {
      title: '回应成员边界',
    })
  ).json();
  const ownerMembership = group.membership.membership_id;
  const invite = async () =>
    await (
      await api(
        stack,
        ownerCookie,
        `/conversations/${group.id}/invitations`,
        'POST',
        { membership_id: ownerMembership },
      )
    ).json();
  const initialInvite = await invite();
  const joined = await (
    await api(stack, peer.cookie, '/group-invitations/accept', 'POST', {
      token: initialInvite.token,
    })
  ).json();
  const old = await (
    await api(
      stack,
      ownerCookie,
      `/conversations/${group.id}/messages`,
      'POST',
      {
        client_message_id: crypto.randomUUID(),
        body: '只属于旧成员历史',
        membership_id: ownerMembership,
      },
    )
  ).json();
  expect(
    (
      await api(
        stack,
        peer.cookie,
        `/conversations/${group.id}/messages/${old.id}/reaction`,
        'PUT',
        { kind: 'like', membership_id: joined.membership.membership_id },
      )
    ).status,
  ).toBe(200);
  await page.goto(`/app/chats/${group.id}`);
  const ownerBubble = page
    .getByRole('log', { name: '聊天消息' })
    .locator(`[data-message-id="${old.id}"]`);
  await expect(
    ownerBubble.getByRole('button', { name: '点赞，1 个回应' }),
  ).toHaveAttribute('aria-pressed', 'false');
  const peerContext = await browser.newContext({ baseURL: stack.origin });
  await peerContext.addCookies([
    {
      name: 'chat_session',
      value: peer.cookie.slice('chat_session='.length),
      domain: 'localhost',
      path: '/',
    },
  ]);
  const peerPage = await peerContext.newPage();
  try {
    await peerPage.goto(`/app/chats/${group.id}`);
    await expect(
      peerPage
        .getByRole('log', { name: '聊天消息' })
        .getByText(old.body, { exact: true }),
    ).toBeVisible();

    expect(
      (
        await api(
          stack,
          ownerCookie,
          `/conversations/${group.id}/members/${peer.user.id}`,
          'DELETE',
          { membership_id: ownerMembership },
        )
      ).status,
    ).toBe(204);
    await expect(peerPage.getByText(old.body, { exact: true })).toHaveCount(0);
    const rejoinInvite = await invite();
    const rejoined = await (
      await api(stack, peer.cookie, '/group-invitations/accept', 'POST', {
        token: rejoinInvite.token,
      })
    ).json();
    await peerPage.goto(`/app/chats/${group.id}`);

    await expect(
      peerPage
        .getByRole('log', { name: '聊天消息' })
        .getByText(old.body, { exact: true }),
    ).toHaveCount(0);
    expect(
      (
        await api(
          stack,
          peer.cookie,
          `/conversations/${group.id}/messages/${old.id}/reaction`,
          'PUT',
          { kind: 'love', membership_id: rejoined.membership.membership_id },
        )
      ).status,
    ).toBe(404);
    await expect(
      ownerBubble.getByRole('button', { name: '点赞，1 个回应' }),
    ).toBeVisible();
    const fresh = await (
      await api(
        stack,
        ownerCookie,
        `/conversations/${group.id}/messages`,
        'POST',
        {
          client_message_id: crypto.randomUUID(),
          body: '新成员可见的回应',
          membership_id: ownerMembership,
        },
      )
    ).json();
    const freshBubble = peerPage
      .getByRole('log', { name: '聊天消息' })
      .locator(`[data-message-id="${fresh.id}"]`);
    await freshBubble.getByRole('button', { name: '添加表情回应' }).click();
    await freshBubble
      .getByRole('button', { name: '喜欢', exact: true })
      .click();
    await expect(
      freshBubble.getByRole('button', { name: '喜欢，1 个回应' }),
    ).toHaveAttribute('aria-pressed', 'true');
  } finally {
    await peerContext.close();
  }
});
