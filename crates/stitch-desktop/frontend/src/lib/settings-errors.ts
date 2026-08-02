/** Map provider / IPC errors to short L1 copy (ADR-025). */

export function friendlyModelError(raw: unknown): string {
  const s = String(raw ?? "").trim();
  if (!s) return "模型连接失败";

  const lower = s.toLowerCase();
  if (
    /authentication fails|incorrect api key|invalid.*api.?key|api key.*invalid|unauthorized|401/.test(
      lower,
    )
  ) {
    return "API Key 无效，请检查后重试";
  }
  if (/model.*(not found|does not exist)|invalid_model|404/.test(lower)) {
    return "模型名称不可用，请更换后重试";
  }
  if (/timeout|timed out|deadline/.test(lower)) {
    return "连接超时，请检查网络或 API 地址";
  }
  if (/dns|enotfound|getaddrinfo|network/.test(lower)) {
    return "无法连接 API 地址，请检查网络与地址";
  }

  const jsonMatch = s.match(/\{[\s\S]*\}/);
  if (jsonMatch) {
    try {
      const j = JSON.parse(jsonMatch[0]) as {
        error?: { message?: string; type?: string };
        message?: string;
      };
      const msg = j?.error?.message || j?.message || "";
      if (msg) return friendlyModelError(msg);
    } catch {
      /* ignore */
    }
  }

  const stripped = s
    .replace(/^模型 API 错误:\s*/i, "")
    .replace(/^模型连接失败:\s*/i, "")
    .replace(/\s+/g, " ")
    .trim();
  if (stripped.length > 100) return "模型连接失败，请检查密钥与地址";
  return stripped || "模型连接失败";
}

export function friendlyMcpError(raw: unknown): string {
  const s = String(raw ?? "").trim();
  if (!s) return "账号连接失败";
  if (/token|401|unauthorized|未填写/i.test(s)) {
    return "Token 无效或未填写，请检查后重试";
  }
  if (s.length > 100) return "账号连接失败，请检查 Token 与服务地址";
  return s.replace(/^PromptStdio 连接失败[^:]*:\s*/i, "").trim() || "账号连接失败";
}

/** Account / library cloud calls (suites · agents · skills list). */
export function isAccountAuthError(raw: unknown): boolean {
  const s = String(raw ?? "");
  return /401|unauthorized|token.*(invalid|missing|未)|未填写|未连接/i.test(s);
}

export function friendlyLibraryError(raw: unknown): string {
  const s = String(raw ?? "").trim();
  if (!s) return "操作失败，请稍后重试";
  if (isAccountAuthError(s)) {
    return "账号未连接或已失效，请先连接 PromptStdio";
  }
  if (/403|forbidden|会员|quota|额度/i.test(s)) {
    return "当前账号无法运行此项，请检查会员或权限";
  }
  if (/404|not found|不存在/i.test(s)) {
    return "未找到该套件或智能体，请核对 ID";
  }
  if (/timeout|timed out|网络|network|dns|enotfound/i.test(s)) {
    return "网络异常，请稍后重试";
  }
  // Never surface raw API / JSON bodies in the UI.
  if (/api error\s*\d+|^\s*\{[\s\S]*"error"/i.test(s) || s.length > 120) {
    return "操作失败，请检查账号与网络后重试";
  }
  return s;
}
