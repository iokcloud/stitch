import { expect, type Page } from "@playwright/test";

/**
 * Patterns that must never appear in *visible* page text (body.innerText).
 * Catches WebView2/flex painting of SvelteKit bootstrap scripts (PITFALLS S-008)
 * and similar shell leaks that data-testid smoke misses.
 */
import { FORBIDDEN_VISIBLE_PATTERNS, AI_SPEAK_PATTERNS } from "./ui-patterns";

/**
 * Visible-layer hygiene after bootstrap. Prefer this over textContent —
 * textContent includes <script> source even when correctly hidden.
 */
export async function assertUiHygiene(page: Page): Promise<void> {
  await expect(page.locator("#app-loader")).toHaveCount(0);

  const visible = await page.locator("body").innerText();
  for (const re of FORBIDDEN_VISIBLE_PATTERNS) {
    expect(visible, `visible UI must not match ${re}`).not.toMatch(re);
  }
  for (const re of AI_SPEAK_PATTERNS) {
    expect(visible, `visible UI must not contain AI-speak: ${re}`).not.toMatch(re);
  }

  // Flex containers can paint <script> with non-zero box even when "executed"
  const paintedScripts = await page.evaluate(() =>
    [...document.querySelectorAll("script")].filter((el) => {
      const r = el.getBoundingClientRect();
      return r.width > 0 && r.height > 0;
    }).length,
  );
  expect(paintedScripts, "no <script> may have a painted box").toBe(0);
}
