/** 禁词/泄漏 pattern 单源（frontend 与 desktop e2e 共用）。 */
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

