import { test, expect, signup } from './fixtures';

test('dialogs retain keyboard focus, dark surfaces and mobile bounds', async ({
  page,
  stack,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signup(page, stack);
  await page.goto('/app/settings');
  await page.getByLabel('外观', { exact: true }).selectOption('dark');
  await page.getByRole('link', { name: '返回聊天', exact: true }).click();

  for (const name of ['发起聊天', '创建或加入群聊']) {
    const trigger = page.getByRole('button', { name, exact: true });
    await trigger.click();
    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
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
    expect(surface.background).toBe('rgb(24, 38, 34)');
    expect(surface.foreground).toBe('rgb(220, 233, 225)');
    expect(surface.left).toBeGreaterThanOrEqual(0);
    expect(surface.right).toBeLessThanOrEqual(390);
    expect(surface.top).toBeGreaterThanOrEqual(0);
    expect(surface.bottom).toBeLessThanOrEqual(844);

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
    await expect(dialog).toBeVisible();
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    await expect(dialog).not.toBeVisible();
    await expect(trigger).toBeFocused();
  }
});
