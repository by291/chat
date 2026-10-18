import { test, expect, signup } from './fixtures';

test('settings navigation follows scroll and preserves drafts across categories', async ({
  page,
  stack,
}) => {
  await signup(page, stack);
  await page.goto('/app/settings');
  await page.getByLabel('昵称', { exact: true }).fill('未保存的昵称');
  const navigation = page.getByRole('navigation', { name: '设置分类' });
  const current = navigation.locator('[aria-current="location"]');
  const categories = [
    { label: '账号安全', id: 'settings-security' },
    { label: '通知外观', id: 'settings-preferences' },
    { label: '应用', id: 'settings-application' },
    { label: '个人资料', id: 'settings-profile' },
  ];

  for (const width of [1280, 390, 320]) {
    await page.setViewportSize({ width, height: 900 });
    await page.locator('.app-content').evaluate((viewport) => {
      viewport.scrollTop = 0;
    });

    const initialTabHeight = (await navigation.boundingBox())!.height;

    for (const category of categories) {
      await navigation
        .getByRole('link', { name: category.label, exact: true })
        .click();

      await expect(current).toHaveText(category.label);
      expect((await navigation.boundingBox())!.height).toBeCloseTo(
        initialTabHeight,
      );
      if (category.id === 'settings-security') {
        const previousCard = await page
          .getByRole('region', { name: '基本资料', exact: true })
          .boundingBox();
        const tabs = await navigation.boundingBox();
        expect(previousCard!.y + previousCard!.height).toBeLessThanOrEqual(
          tabs!.y + tabs!.height,
        );
      }
      if (
        category.id === 'settings-security' ||
        category.id === 'settings-profile'
      ) {
        const heading = await page.locator(`#${category.id}`).boundingBox();
        const tabs = await navigation.boundingBox();
        expect(Math.round(heading!.y)).toBe(Math.round(tabs!.y + tabs!.height));
      }
      await expect(navigation).toBeInViewport();
      await expect(
        page.getByRole('heading', { name: '个人设置', exact: true }),
      ).not.toBeInViewport();
      await expect(page.locator(`#${category.id}`)).toBeInViewport();
      await expect
        .poll(async () => {
          const heading = await page.locator(`#${category.id}`).boundingBox();
          const tabs = await navigation.boundingBox();
          return Math.round(heading!.y) >= Math.round(tabs!.y + tabs!.height);
        })
        .toBe(true);
      await expect(page.getByLabel('昵称', { exact: true })).toHaveValue(
        '未保存的昵称',
      );
    }

    // The sticky surface spans the scroll viewport while its links align with content.
    const viewportBounds = await page
      .locator('.app-content')
      .evaluate((element) => {
        const bounds = element.getBoundingClientRect();
        return { left: bounds.left, width: element.clientWidth };
      });
    const navigationBounds = await navigation.boundingBox();
    const firstTabBounds = await navigation
      .getByRole('link')
      .first()
      .boundingBox();
    const profileBounds = await page.locator('#settings-profile').boundingBox();
    expect(navigationBounds!.x).toBeCloseTo(viewportBounds.left);
    expect(navigationBounds!.width).toBeCloseTo(viewportBounds.width);
    expect(firstTabBounds!.x).toBeCloseTo(profileBounds!.x);

    // Scroll a card beneath the tabs and check that the whole sticky band covers it.
    await page
      .getByRole('region', { name: '头像', exact: true })
      .evaluate((card) => {
        const viewport = card.closest('.app-content')!;
        viewport.scrollTop +=
          card.getBoundingClientRect().top -
          viewport.getBoundingClientRect().top +
          100;
      });
    await expect
      .poll(() =>
        navigation.evaluate((element) => {
          const bounds = element.getBoundingClientRect();
          const y = bounds.top + bounds.height / 2;
          return [0.01, 0.99].every((ratio) =>
            element.contains(
              document.elementFromPoint(bounds.left + bounds.width * ratio, y),
            ),
          );
        }),
      )
      .toBe(true);

    // Scroll independently of the links so a stale URL hash cannot drive selection.
    await page
      .locator('#settings-security')
      .evaluate((heading) => heading.scrollIntoView());

    await expect(current).toHaveText('账号安全');
    await expect(page).toHaveURL(/#settings-profile$/);
    const tabRows = await navigation
      .getByRole('link')
      .evaluateAll((links) =>
        links.map((link) => link.getBoundingClientRect().top),
      );
    expect(new Set(tabRows).size).toBe(1);

    await page.locator('.app-content').evaluate(async (viewport) => {
      viewport.scrollTop = viewport.scrollHeight;
      // Wait for the scroll event and its scheduled highlight update.
      await new Promise<void>((resolve) => {
        requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
      });
    });

    // Reaching the end must not override the section at the reading position.
    const readingSection = await navigation.evaluate((element) => {
      const tabs = element.getBoundingClientRect();
      return document
        .elementFromPoint(tabs.left + tabs.width / 2, tabs.bottom + 1)
        ?.closest('.settings-group')
        ?.getAttribute('aria-labelledby');
    });
    expect(readingSection).toBeTruthy();
    expect(readingSection).not.toBe('settings-application');
    await expect(current).toHaveAttribute('href', `#${readingSection}`);
    const installCard = await page
      .getByRole('region', { name: '安装应用', exact: true })
      .boundingBox();
    expect(900 - installCard!.y - installCard!.height).toBeGreaterThanOrEqual(
      0,
    );
    expect(900 - installCard!.y - installCard!.height).toBeLessThan(40);
  }
});
