/** 诊断：场景面板四个菜单项的对齐 */
import { bootChat, newSession } from "../helpers/chat-desktop";

describe("diag menu align", () => {
  it("screenshots the library sidebar", async () => {
    await bootChat();
    await newSession();
    // 切到场景（library）tab
    await $('[data-testid="library-tab"]').click();
    await browser.pause(600);
    const info = await browser.execute(() => {
      const nav = document.querySelector(".side-nav");
      const btns = [...document.querySelectorAll(".side-seg-library .side-seg-btn")].map((b) => {
        const r = b.getBoundingClientRect();
        const svg = b.querySelector("svg")?.getBoundingClientRect();
        const span = b.querySelector("span")?.getBoundingClientRect();
        return {
          label: b.textContent?.trim(),
          btn: { y: Math.round(r.y), h: Math.round(r.height) },
          svg: svg ? { y: Math.round(svg.y), h: Math.round(svg.height) } : null,
          span: span ? { y: Math.round(span.y), h: Math.round(span.height) } : null,
        };
      });
      return { navH: nav?.getBoundingClientRect().height, btns };
    });
    console.log("[ALIGN]", JSON.stringify(info));
    await browser.saveScreenshot("./artifacts/diag-menu-align.png");
  });
});
