import { marked } from "marked";
import hljs from "highlight.js/lib/core";
import javascript from "highlight.js/lib/languages/javascript";
import typescript from "highlight.js/lib/languages/typescript";
import rust from "highlight.js/lib/languages/rust";
import python from "highlight.js/lib/languages/python";
import bash from "highlight.js/lib/languages/bash";
import json from "highlight.js/lib/languages/json";
import markdown from "highlight.js/lib/languages/markdown";
import xml from "highlight.js/lib/languages/xml";
import css from "highlight.js/lib/languages/css";
import yaml from "highlight.js/lib/languages/yaml";
import sql from "highlight.js/lib/languages/sql";

hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("js", javascript);
hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("ts", typescript);
hljs.registerLanguage("rust", rust);
hljs.registerLanguage("rs", rust);
hljs.registerLanguage("python", python);
hljs.registerLanguage("py", python);
hljs.registerLanguage("bash", bash);
hljs.registerLanguage("sh", bash);
hljs.registerLanguage("shell", bash);
hljs.registerLanguage("json", json);
hljs.registerLanguage("markdown", markdown);
hljs.registerLanguage("md", markdown);
hljs.registerLanguage("html", xml);
hljs.registerLanguage("xml", xml);
hljs.registerLanguage("css", css);
hljs.registerLanguage("yaml", yaml);
hljs.registerLanguage("yml", yaml);
hljs.registerLanguage("sql", sql);

marked.setOptions({
  breaks: true,
  gfm: true,
});

const ALLOWED_TAGS = new Set([
  "p",
  "br",
  "hr",
  "strong",
  "b",
  "em",
  "i",
  "u",
  "s",
  "del",
  "code",
  "pre",
  "blockquote",
  "ul",
  "ol",
  "li",
  "a",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "table",
  "thead",
  "tbody",
  "tr",
  "th",
  "td",
  "span",
  "div",
]);

export function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

// Cache rendered Markdown — identical assistant content (e.g. re-rendered
// after any store touch) skips marked.parse + sanitize + hljs entirely.
const markdownCache = new Map<string, string>();
const MARKDOWN_CACHE_MAX = 200;

export function renderMarkdown(src: string): string {
  try {
    const cached = markdownCache.get(src);
    if (cached !== undefined) return cached;
    const html = marked.parse(src, { async: false }) as string;
    const out = highlightInHtml(sanitizeHtml(html));
    markdownCache.set(src, out);
    if (markdownCache.size > MARKDOWN_CACHE_MAX) {
      const first = markdownCache.keys().next().value;
      if (first !== undefined) markdownCache.delete(first);
    }
    return out;
  } catch {
    return `<pre>${escapeHtml(src)}</pre>`;
  }
}

/** Strip scripts / event handlers / javascript: URLs before `{@html}`. */
export function sanitizeHtml(html: string): string {
  if (typeof document === "undefined") {
    return html
      .replace(/<script[\s\S]*?>[\s\S]*?<\/script>/gi, "")
      .replace(/\son\w+\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)/gi, "")
      .replace(/javascript:/gi, "");
  }
  const wrap = document.createElement("div");
  wrap.innerHTML = html;
  const walk = (node: Node) => {
    const children = Array.from(node.childNodes);
    for (const child of children) {
      if (child.nodeType === Node.ELEMENT_NODE) {
        const el = child as HTMLElement;
        const tag = el.tagName.toLowerCase();
        if (!ALLOWED_TAGS.has(tag) || tag === "script" || tag === "style" || tag === "iframe") {
          el.replaceWith(...Array.from(el.childNodes));
          continue;
        }
        for (const attr of Array.from(el.attributes)) {
          const name = attr.name.toLowerCase();
          const value = attr.value;
          if (name.startsWith("on") || name === "srcdoc") {
            el.removeAttribute(attr.name);
            continue;
          }
          if ((name === "href" || name === "src") && /^\s*javascript:/i.test(value)) {
            el.removeAttribute(attr.name);
          }
        }
        if (tag === "a") {
          el.setAttribute("rel", "noopener noreferrer");
          el.setAttribute("target", "_blank");
        }
        walk(el);
      }
    }
  };
  walk(wrap);
  return wrap.innerHTML;
}

function highlightInHtml(html: string): string {
  if (typeof document === "undefined") return html;
  const wrap = document.createElement("div");
  wrap.innerHTML = html;
  wrap.querySelectorAll("pre code").forEach((block) => {
    try {
      hljs.highlightElement(block as HTMLElement);
    } catch {
      /* ignore */
    }
  });
  // Code blocks: lang badge + copy affordance shell (click handled in MessageBubble).
  wrap.querySelectorAll("pre").forEach((pre) => {
    if (pre.parentElement?.classList.contains("md-code-block")) return;
    pre.classList.add("md-block-scroll");
    const code = pre.querySelector("code");
    const langClass = [...(code?.classList ?? [])].find((c) => c.startsWith("language-"));
    const lang = langClass ? langClass.replace("language-", "") : "code";
    const shell = document.createElement("div");
    shell.className = "md-code-block";
    shell.dataset.format = "code";
    const head = document.createElement("div");
    head.className = "md-code-head";
    const langEl = document.createElement("span");
    langEl.className = "md-code-lang";
    langEl.textContent = lang;
    const copyBtn = document.createElement("button");
    copyBtn.type = "button";
    copyBtn.className = "md-code-copy";
    copyBtn.setAttribute("aria-label", "复制代码");
    copyBtn.setAttribute("title", "复制代码");
    copyBtn.innerHTML =
      '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><rect x="9" y="9" width="11" height="11" rx="1.5"/><path d="M5 15V5.5A1.5 1.5 0 016.5 4H15"/></svg>';
    head.appendChild(langEl);
    head.appendChild(copyBtn);
    pre.parentNode?.insertBefore(shell, pre);
    shell.appendChild(head);
    shell.appendChild(pre);
  });
  wrap.querySelectorAll("a[href]").forEach((a) => {
    a.classList.add("md-link");
    a.setAttribute("data-format", "link");
  });
  wrap.querySelectorAll("table").forEach((table) => {
    if (table.parentElement?.classList.contains("md-table-scroll")) return;
    const scroller = document.createElement("div");
    scroller.className = "md-table-scroll";
    table.parentNode?.insertBefore(scroller, table);
    scroller.appendChild(table);
  });
  return wrap.innerHTML;
}
