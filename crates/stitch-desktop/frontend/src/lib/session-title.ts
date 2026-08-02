import { RECOMMENDED_SCENES } from "./scenes";
import { matchMatureScene } from "./mature-scenes";

/**
 * Short sidebar title from the first user message (rule-based, not LLM).
 * Prefers known scene names over raw long prompts; aims for ~12–16 CJK chars.
 */
export function summarizeSessionTitle(raw: string): string {
  const trimmed = (raw || "").trim();
  if (!trimmed) return "新会话";

  const rec = RECOMMENDED_SCENES.find(
    (s) => trimmed === s.prompt.trim() || trimmed.startsWith(s.prompt.trim()),
  );
  if (rec) return rec.title;

  const mature = matchMatureScene(trimmed);
  if (mature) return mature.title;

  let t = trimmed
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`[^`]*`/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  if (!t) return "新会话";
  // Suite / agent runners prefix the title in the user bubble
  t = t.replace(/^执行(?:套件|智能体)[：:]\s*/u, "").trim() || t;
  // Prefer first sentence / clause
  const cut = t.search(/[。！？!?\n]/);
  if (cut > 4 && cut < 48) t = t.slice(0, cut);
  // Drop leading politeness only (keep 「对最近…」 intact for scene match above)
  t = t.replace(/^(?:请|帮我|帮忙)(?:把|将|对|关于)?\s*/u, "").trim() || t;
  // Common short intents — keep readable, don't look like a clipped prompt dump
  if (/^审查本次改动/.test(t)) return "审查本次改动";
  const max = 16;
  if ([...t].length <= max) return t;
  return [...t].slice(0, max).join("") + "…";
}
