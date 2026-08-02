import { expect, type Page } from "@playwright/test";

/**
 * Patterns that must never appear in *visible* page text (body.innerText).
 * Catches WebView2/flex painting of SvelteKit bootstrap scripts (PITFALLS S-008)
 * and similar shell leaks that data-testid smoke misses.
 */
export const FORBIDDEN_VISIBLE_PATTERNS: RegExp[] = [
  /__sveltekit_/,
  /document\.currentScript/,
  /%sveltekit\./,
  /Promise\.all\(\s*\[\s*import\s*\(/,
  /import\s*\(\s*["']\.\/_app\//,
];

/** AI 味营销腔禁词（ADR-025 补充 · copy-tone 词表镜像）——只收最典型的营销词，避免误伤功能语境。 */
export const AI_SPEAK_PATTERNS: RegExp[] = [
  /赋能/,
  /一站式/,
  /极致/,
  /焕新/,
  /助力/,
  /打造/,
  /引领/,
  /丝滑/,
  /沉浸式/,
  /智享/,
];

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
