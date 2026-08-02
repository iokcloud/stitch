/**
 * Normalize pasted OpenAI-compatible URLs to an API root.
 * e.g. `…/v1/chat/completions` → `…/v1`
 */
export function normalizeOpenAiCompatibleBase(raw: string): string {
  let s = raw.trim();
  if (!s) return s;
  s = s.replace(/\/+$/, "");
  s = s.replace(/\/chat\/completions$/i, "");
  s = s.replace(/\/completions$/i, "");
  s = s.replace(/\/+$/, "");
  return s;
}
