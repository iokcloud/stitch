/**
 * Mirror of frontend/e2e/helpers/ui-hygiene.ts (Layer A).
 * Keep patterns in sync — Layer B cannot import frontend helpers easily.
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

type HygieneProbe = {
  loaderPresent: boolean;
  text: string;
  paintedScripts: number;
};

/** Returns first matching leak description, or null if clean. */
export async function findVisibleUiLeak(): Promise<string | null> {
  const result = (await browser.execute(() => {
    const loader = document.getElementById("app-loader");
    const text = document.body?.innerText ?? "";
    const paintedScripts = [...document.querySelectorAll("script")].filter((el) => {
      const r = el.getBoundingClientRect();
      return r.width > 0 && r.height > 0;
    }).length;
    return {
      loaderPresent: !!loader,
      text,
      paintedScripts,
    };
  })) as HygieneProbe;

  if (result.loaderPresent) {
    return "#app-loader still present";
  }

  for (const re of FORBIDDEN_VISIBLE_PATTERNS) {
    if (re.test(result.text)) {
      return `visible text matched ${re}`;
    }
  }
  for (const re of AI_SPEAK_PATTERNS) {
    if (re.test(result.text)) {
      return `visible text matched AI-speak ${re}`;
    }
  }
  if (result.paintedScripts > 0) {
    return `${result.paintedScripts} <script> element(s) have a painted box`;
  }
  return null;
}
