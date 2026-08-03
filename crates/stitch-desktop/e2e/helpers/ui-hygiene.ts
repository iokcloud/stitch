/**
 * Mirror of frontend/e2e/helpers/ui-hygiene.ts (Layer A).
 * Keep patterns in sync — Layer B cannot import frontend helpers easily.
 */
import { FORBIDDEN_VISIBLE_PATTERNS, AI_SPEAK_PATTERNS } from "../../frontend/e2e/helpers/ui-patterns";

type HygieneProbe = {
  loaderPresent: boolean;
  text: string;
  paintedScripts: number;
};

/** Returns first matching leak description, or null if clean. */
export async function findVisibleUiLeak(): Promise<string | null> {
  const result = (await browser.execute(() => {
    const loader = document.getElementById("app-loader");
    // 产品文案检查只扫 UI 壳——克隆 body 后删除脚本/会话内容区再取
    // innerText：① 不含 script 源码（textContent 会把 __sveltekit_ 等
    // 注入文本带进来）② 排除用户数据（用户消息/助手回复是用户内容，
    // 营销词检查不该误伤，如助手自我介绍含「打造」）。
    const clone = document.body.cloneNode(true) as HTMLElement;
    clone
      .querySelectorAll('script, style, template, [role="log"], .message, .md-content')
      .forEach((n) => n.remove());
    const text = clone.innerText ?? "";
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
