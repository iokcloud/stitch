/**
 * Cached membership probe for mature-scene gates (G1 soft / G2 hard).
 * Never gates chat tools — only official paid_pool scene chrome.
 */
import type { MembershipSnapshot } from "./types";
import * as ipc from "./ipc";

const TTL_MS = 10 * 60 * 1000;

let cached: MembershipSnapshot | null = null;
let cachedAt = 0;

export function clearMembershipCache() {
  cached = null;
  cachedAt = 0;
}

export async function fetchMembership(force = false): Promise<MembershipSnapshot> {
  const now = Date.now();
  if (!force && cached && now - cachedAt < TTL_MS) {
    return cached;
  }
  try {
    cached = await ipc.getMembership();
  } catch {
    cached = {
      token_set: false,
      is_member: false,
      status: "unknown",
      pricing_url: "https://www.promptstdio.com/pricing",
    };
  }
  cachedAt = Date.now();
  return cached;
}
