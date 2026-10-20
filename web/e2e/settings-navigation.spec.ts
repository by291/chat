import { test, expect, signup } from './fixtures';

test('settings categories preserve drafts and follow direct links and browser history', async ({
  page,
  stack,
}) => {
  await signup(page, stack);
  await page.goto('/app/settings');
  await page.getByLabel('当前密码', { exact: true }).fill('unsaved-password');
  const navigation = page.getByRole('navigation', { name: '设置分类' });
  const security = page.getByRole('region', { name: '账号安全', exact: true });

  await expect(security).toBeVisible();
  await navigation.getByRole('link', { name: '通知外观' }).click();
  await expect(security).toBeHidden();
  await expect(page).toHaveURL(/#settings-preferences$/);

  await page.goBack();
  await expect(page.getByLabel('当前密码', { exact: true })).toHaveValue(
    'unsaved-password',
  );
  await expect(page.getByLabel('当前密码', { exact: true })).toBeVisible();

  for (const width of [1280, 390, 320]) {
    await page.setViewportSize({ width, height: 900 });
    if (width <= 700 && new URL(page.url()).hash) {
      await page.getByRole('button', { name: '返回设置列表' }).click();
      await expect(navigation).toBeVisible();
      await expect(
        page.getByRole('navigation', { name: '主导航' }),
      ).toBeVisible();
    }
    await navigation.getByRole('link', { name: '通知外观' }).click();
    await expect(page.getByLabel('外观', { exact: true })).toBeInViewport();
    await expect(security).toBeHidden();
    if (width <= 700) {
      await expect(navigation).toBeHidden();
      await expect(
        page.getByRole('navigation', { name: '主导航' }),
      ).toBeHidden();
      const back = page.getByRole('button', { name: '返回设置列表' });
      expect((await back.boundingBox())?.y).toBeLessThan(30);
      await back.click();
    }
    await navigation.getByRole('link', { name: '账号安全' }).click();
    await expect(security).toBeVisible();
    await expect(page.locator('#settings-security')).toBeFocused();
    await expect(page.getByLabel('当前密码', { exact: true })).toHaveValue(
      'unsaved-password',
    );
    expect(
      await page.locator('body').evaluate((body) => body.scrollWidth),
    ).toBeLessThanOrEqual(width);
    await page.screenshot({
      path: test.info().outputPath(`settings-${width}.png`),
    });
  }

  await page.goto('/app/settings#settings-application');
  await expect(page.getByRole('region', { name: '安装应用' })).toBeVisible();
  await expect(page.getByLabel('当前密码', { exact: true })).toBeHidden();
});
