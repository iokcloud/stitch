/**
 * Subsequence fuzzy matching for the command palette.
 * Higher score = better match; null = query is not a subsequence of target.
 * Contiguous runs and word/segment starts score higher.
 */
export function fuzzyScore(query: string, target: string): number | null {
    const q = query.trim().toLowerCase();
    const t = target.toLowerCase();
    if (!q) return 0;
    let score = 0;
    let ti = 0;
    let lastMatch = -2;
    let firstIndex = -1;
    for (let qi = 0; qi < q.length; qi++) {
        const idx = t.indexOf(q[qi], ti);
        if (idx < 0) return null;
        if (firstIndex < 0) firstIndex = idx;
        if (idx === lastMatch + 1) score += 10;
        if (idx === 0 || /[\s·:：\-_/]/.test(t[idx - 1])) score += 6;
        score += 1;
        lastMatch = idx;
        ti = idx + 1;
    }
    score -= firstIndex * 0.5;
    score -= (t.length - q.length) * 0.05;
    return score;
}

/** Best fuzzy score across a title and optional keyword aliases. */
export function fuzzyBest(query: string, title: string, keywords: string[] = []): number | null {
    let best = fuzzyScore(query, title);
    for (const k of keywords) {
        const s = fuzzyScore(query, k);
        if (s !== null && (best === null || s > best)) best = s;
    }
    return best;
}
