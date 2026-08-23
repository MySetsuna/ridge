import { expect, test, type Page } from '@playwright/test';

const SETTINGS_MODULE_URL = '/src/lib/stores/settings.ts';
const requestedBootTimeout = Number(process.env.RIDGE_E2E_BOOT_TIMEOUT_MS);
const BOOT_TIMEOUT_MS = Number.isFinite(requestedBootTimeout) && requestedBootTimeout >= 5_000
  ? requestedBootTimeout
  : 30_000;

async function bootSpa(page: Page, teammateEnabled: boolean): Promise<void> {
  await page.addInitScript((enabled) => {
    localStorage.setItem('ridge-settings', JSON.stringify({ teammateEnabled: enabled }));
  }, teammateEnabled);
  try {
    await page.goto('/?e2e=1', { waitUntil: 'domcontentloaded' });
  } catch (error) {
    // A cold Vite optimize pass reloads the page and aborts the original
    // navigation. The browser is already following that reload; a second goto
    // races it and can keep the app blank indefinitely.
    if (!String(error).includes('net::ERR_ABORTED')) throw error;
  }
  await expect(
    page.getByRole('complementary', { name: 'Primary navigation' }),
  ).toBeVisible({ timeout: BOOT_TIMEOUT_MS });
  await page.waitForFunction(
    () => {
      const loader = document.getElementById('brand-loader');
      return !loader || getComputedStyle(loader).display === 'none';
    },
    null,
    { timeout: BOOT_TIMEOUT_MS },
  );
}

async function setTeammateEnabled(page: Page, enabled: boolean): Promise<void> {
  await page.evaluate(async ([moduleUrl, value]) => {
    const settings = await import(/* @vite-ignore */ moduleUrl);
    settings.setSetting('teammateEnabled', value);
  }, [SETTINGS_MODULE_URL, enabled] as const);
}

test.describe("Agent's Commune visibility", () => {
  test('entry remains visible while disabled and click enables the empty panel', async ({ page }) => {
    await bootSpa(page, false);

    const entry = page.getByTestId('commune-entry');
    await expect(entry).toBeVisible();
    await expect(entry).toHaveAttribute('title', "Agent's Commune（点击启用）");
    await expect(page.getByTestId('commune-panel')).toHaveCount(0);

    await entry.click();

    const panel = page.getByTestId('commune-panel');
    await expect(panel).toBeVisible();
    await expect.poll(() => panel.evaluate((element) => element.getBoundingClientRect().width))
      .toBeGreaterThanOrEqual(360);
    await expect(panel).toContainText("Agent's Commune");
    await expect(panel).toContainText('暂无成员');
    await expect.poll(() => page.evaluate(() => {
      const raw = localStorage.getItem('ridge-settings');
      return raw ? JSON.parse(raw).teammateEnabled : undefined;
    })).toBe(true);
  });

  test('disabling closes the panel but never removes its entry', async ({ page }) => {
    await bootSpa(page, true);
    await page.getByTestId('commune-entry').click();
    await expect(page.getByTestId('commune-panel')).toBeVisible();

    await setTeammateEnabled(page, false);

    await expect(page.getByTestId('commune-panel')).toHaveCount(0);
    await expect(page.getByTestId('commune-entry')).toBeVisible();
    await expect(page.getByTestId('commune-entry')).toHaveAttribute(
      'title',
      "Agent's Commune（点击启用）",
    );
  });

  test('members, groups, and history tabs are reachable in the empty state', async ({ page }) => {
    await bootSpa(page, true);
    await page.getByTestId('commune-entry').click();
    const panel = page.getByTestId('commune-panel');

    const members = page.getByTestId('commune-tab-members');
    const groups = page.getByTestId('commune-tab-groups');
    const history = page.getByTestId('commune-tab-history');
    await expect(members).toBeVisible();
    await expect(groups).toBeVisible();
    await expect(history).toBeVisible();

    await groups.click();
    await expect(groups).toHaveClass(/bg-\[var\(--rg-accent\)\]/);

    await history.click();
    await expect(history).toHaveClass(/bg-\[var\(--rg-accent\)\]/);
    await expect(panel).toContainText('暂无历史会话');

    await members.click();
    await expect(members).toHaveClass(/bg-\[var\(--rg-accent\)\]/);
    await expect(panel).toContainText('暂无成员');
  });
});
