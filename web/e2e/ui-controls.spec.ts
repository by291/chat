import { test, expect, signup } from './fixtures';

test('dialogs retain keyboard focus, dark surfaces and mobile bounds', async ({
  page,
  stack,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signup(page, stack);
  await page.goto('/app/settings#settings-preferences');
  await page.getByLabel('外观', { exact: true }).selectOption('dark');
  await page.getByRole('button', { name: '返回设置列表' }).click();
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('link', { name: '聊天', exact: true })
    .click();

  for (const name of ['创建群聊', '加入群聊']) {
    const trigger = page.getByRole('button', {
      name: '群聊操作',
      exact: true,
    });
    await trigger.click();
    await expect(page.getByRole('menuitem')).toHaveCount(2);
    await page.screenshot({
      path: test.info().outputPath('conversation-menu-mobile-dark.png'),
      animations: 'disabled',
    });
    await page.getByRole('menuitem', { name, exact: true }).click();
    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    if (name === '加入群聊') {
      await expect(
        dialog.getByLabel('邀请链接或口令', { exact: true }),
      ).toBeVisible();
      await expect(
        dialog.getByRole('button', { name: '创建群聊', exact: true }),
      ).toHaveCount(0);
    }
    await expect
      .poll(() =>
        dialog.evaluate((element) => element.contains(document.activeElement)),
      )
      .toBe(true);

    const surface = await dialog.evaluate((element) => {
      const style = getComputedStyle(element);
      const bounds = element.getBoundingClientRect();
      return {
        background: style.backgroundColor,
        foreground: style.color,
        left: bounds.left,
        right: bounds.right,
        top: bounds.top,
        bottom: bounds.bottom,
      };
    });
    expect(surface.background).toBe('rgb(32, 42, 44)');
    expect(surface.foreground).toBe('rgb(230, 240, 237)');
    expect(surface.left).toBeGreaterThanOrEqual(0);
    expect(surface.right).toBeLessThanOrEqual(390);
    expect(surface.top).toBeGreaterThanOrEqual(0);
    expect(surface.bottom).toBeLessThanOrEqual(844);
    if (name === '加入群聊') {
      await page.screenshot({
        path: test.info().outputPath('join-group-mobile-dark.png'),
        animations: 'disabled',
      });
    }

    for (let index = 0; index < 8; index++) {
      await page.keyboard.press('Tab');
      expect(
        await dialog.evaluate((element) =>
          element.contains(document.activeElement),
        ),
      ).toBe(true);
    }

    await page.keyboard.press('Escape');
    await expect(dialog).not.toBeVisible();
    await expect(trigger).toBeFocused();

    await trigger.press('Enter');
    await expect(page.getByRole('menuitem').first()).toBeFocused();
    await page.keyboard.press('Home');
    if (name === '加入群聊') await page.keyboard.press('End');
    await expect(
      page.getByRole('menuitem', { name, exact: true }),
    ).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(dialog).toBeVisible();
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    await expect(dialog).not.toBeVisible();
    await expect(trigger).toBeFocused();
  }
  await page.goto('/app/settings#settings-preferences');
  await page.getByLabel('外观', { exact: true }).selectOption('light');
  await page.getByRole('button', { name: '返回设置列表' }).click();
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('link', { name: '聊天', exact: true })
    .click();
  await page.setViewportSize({ width: 1280, height: 844 });
  const trigger = page.getByRole('button', { name: '群聊操作', exact: true });
  await trigger.click();
  await expect(page.getByRole('menu')).toBeVisible();
  await page.screenshot({
    path: test.info().outputPath('conversation-menu-desktop.png'),
    animations: 'disabled',
  });
  await page.keyboard.press('Escape');
  await expect(page.getByRole('menu')).not.toBeVisible();
  await expect(trigger).toBeFocused();
});
