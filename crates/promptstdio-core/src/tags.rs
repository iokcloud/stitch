//! 标签 taxonomy：canonical key + i18n label + 写入归一化。
//!
//! DB / MCP / API 存 **key**；Web UI 按 [`TagLocale`] 显示 label。
//! 词表：`docs/schemas/tag-taxonomy.v1.json`

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::Deserialize;

const TAXONOMY_JSON: &str = include_str!("../schemas/tag-taxonomy.v1.json");

/// 标签展示语言（仅 zh-CN / en）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TagLocale {
    #[default]
    ZhCn,
    En,
}

impl TagLocale {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::En => "en",
        }
    }

    pub fn html_lang(self) -> &'static str {
        self.as_str()
    }

    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "en" | "en-us" | "en-gb" => Self::En,
            _ => Self::ZhCn,
        }
    }

    /// `lang` query/cookie 优先，其次 `Accept-Language` 前缀。
    pub fn from_request(lang: Option<&str>, accept_language: Option<&str>) -> Self {
        if let Some(l) = lang.filter(|s| !s.trim().is_empty()) {
            return Self::parse(l);
        }
        if let Some(header) = accept_language {
            let lower = header.to_ascii_lowercase();
            if lower.starts_with("en") {
                return Self::En;
            }
        }
        Self::ZhCn
    }
}

#[derive(Debug, Deserialize)]
struct TagTaxonomyFile {
    max_count: i64,
    tags: HashMap<String, TagEntry>,
}

#[derive(Debug, Deserialize)]
struct TagEntry {
    labels: HashMap<String, String>,
    aliases: Vec<String>,
    #[serde(default)]
    harvest: bool,
}

struct TagTaxonomy {
    max_count: i64,
    tags: HashMap<String, TagEntry>,
    alias_to_key: HashMap<String, String>,
    harvest_keys: Vec<String>,
}

static TAXONOMY: OnceLock<TagTaxonomy> = OnceLock::new();

fn taxonomy() -> &'static TagTaxonomy {
    TAXONOMY.get_or_init(|| {
        let file: TagTaxonomyFile =
            serde_json::from_str(TAXONOMY_JSON).expect("parse tag-taxonomy.v1.json");

        let mut alias_to_key = HashMap::new();
        let mut harvest_keys = Vec::new();

        for (key, entry) in &file.tags {
            alias_to_key.insert(alias_key(key), key.clone());
            for alias in &entry.aliases {
                alias_to_key.insert(alias_key(alias), key.clone());
            }
            if entry.harvest {
                harvest_keys.push(key.clone());
            }
        }
        harvest_keys.sort();

        TagTaxonomy {
            max_count: file.max_count,
            tags: file.tags,
            alias_to_key,
            harvest_keys,
        }
    })
}

fn alias_key(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_ascii() {
        trimmed.to_ascii_lowercase()
    } else {
        trimmed.to_string()
    }
}

/// MCP / 复利规则：标签数量上限。
pub fn max_tag_count() -> i64 {
    taxonomy().max_count
}

/// 复利白名单（canonical keys）。
pub fn harvest_allowed_keys() -> Vec<String> {
    taxonomy().harvest_keys.clone()
}

/// 所有受控 taxonomy keys（有序）。
pub fn catalog_keys() -> Vec<String> {
    let mut keys: Vec<_> = taxonomy().tags.keys().cloned().collect();
    keys.sort();
    keys
}

/// 将原始输入归一化为 canonical key；未知标签原样保留（trim 后）。
pub fn normalize_tag(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(key) = taxonomy().alias_to_key.get(&alias_key(trimmed)) {
        return key.clone();
    }
    trimmed.to_string()
}

/// 归一化、去重、去空；可选上限（`None` = 不截断）。
pub fn normalize_tags<I>(tags: I, max_count: Option<usize>) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let limit = max_count.unwrap_or(usize::MAX);

    for tag in tags {
        let key = normalize_tag(&tag);
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.insert(key.clone());
        out.push(key);
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// 逗号分隔标签（Web 表单）。
pub fn parse_csv_tags(raw: Option<&str>) -> Option<Vec<String>> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let tags: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if tags.is_empty() {
        None
    } else {
        Some(normalize_tags(tags, None))
    }
}

/// MCP / 剪贴板：逗号分隔或 JSON 数组。
pub fn parse_tags_input(raw: Option<&str>) -> Option<Vec<String>> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with('[') {
        let decoded: serde_json::Value = serde_json::from_str(raw).ok()?;
        let arr = decoded.as_array()?;
        let tags: Vec<String> = arr
            .iter()
            .filter_map(|v| {
                v.as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            })
            .collect();
        return if tags.is_empty() {
            None
        } else {
            Some(normalize_tags(tags, None))
        };
    }
    parse_csv_tags(Some(raw))
}

/// JSON 数组 tags（剪贴板契约）。
pub fn normalize_tags_from_json(value: Option<&serde_json::Value>) -> Vec<String> {
    let Some(serde_json::Value::Array(arr)) = value else {
        return vec![];
    };
    let tags: Vec<String> = arr
        .iter()
        .filter_map(|v| {
            v.as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .collect();
    normalize_tags(tags, None)
}

/// 展示 label；未知 key fallback 为 key 本身。
pub fn tag_label(key: &str, locale: TagLocale) -> String {
    let locale_str = locale.as_str();
    if let Some(entry) = taxonomy().tags.get(key) {
        if let Some(label) = entry.labels.get(locale_str) {
            return label.clone();
        }
        if let Some(fallback) = entry.labels.get("zh-CN") {
            return fallback.clone();
        }
    }
    key.to_string()
}

/// 列表/详情：批量 label。
pub fn tag_labels(keys: &[String], locale: TagLocale) -> Vec<String> {
    keys.iter().map(|k| tag_label(k, locale)).collect()
}

/// 筛选：stored 与 filter 是否同一 canonical key（兼容历史中文 tag）。
pub fn tags_match_filter(stored: &str, filter: &str) -> bool {
    normalize_tag(stored) == normalize_tag(filter)
}

/// DB 筛选变体：key + 全部 alias + 原始 filter（兼容未迁移数据）。
pub fn filter_variants(filter: &str) -> Vec<String> {
    let key = normalize_tag(filter);
    let mut set = HashSet::new();
    set.insert(key.clone());
    set.insert(filter.trim().to_string());
    if let Some(entry) = taxonomy().tags.get(&key) {
        for alias in &entry.aliases {
            set.insert(alias.clone());
        }
    }
    set.into_iter().collect()
}

/// 聚合标签云计数（按 canonical key 合并）。
pub fn aggregate_tag_counts(items: &[(String, i64)]) -> Vec<(String, i64)> {
    let mut map: HashMap<String, i64> = HashMap::new();
    for (tag, count) in items {
        let key = normalize_tag(tag);
        if key.is_empty() {
            continue;
        }
        *map.entry(key).or_default() += count;
    }
    let mut out: Vec<_> = map.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// tags 展示为逗号分隔 label（运营导入预览等）。
pub fn tags_display_labels(keys: &[String], locale: TagLocale) -> String {
    tag_labels(keys, locale).join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_alias_maps_to_key() {
        assert_eq!(normalize_tag("工作流"), "workflow");
        assert_eq!(normalize_tag("元规范"), "meta");
    }

    #[test]
    fn english_key_is_idempotent() {
        assert_eq!(normalize_tag("workflow"), "workflow");
        assert_eq!(normalize_tag("Workflow"), "workflow");
    }

    #[test]
    fn unknown_tag_preserved() {
        assert_eq!(normalize_tag("my-custom"), "my-custom");
    }

    #[test]
    fn dedupe_on_normalize() {
        let out = normalize_tags(
            vec!["工作流".into(), "workflow".into(), "meta".into()],
            None,
        );
        assert_eq!(out, vec!["workflow", "meta"]);
    }

    #[test]
    fn tag_label_bilingual() {
        assert_eq!(tag_label("workflow", TagLocale::ZhCn), "工作流");
        assert_eq!(tag_label("workflow", TagLocale::En), "Workflow");
    }

    #[test]
    fn tags_match_filter_merges_locales() {
        assert!(tags_match_filter("工作流", "workflow"));
    }

    #[test]
    fn harvest_allowed_includes_core_keys() {
        let allowed = harvest_allowed_keys();
        assert!(allowed.contains(&"workflow".to_string()));
        assert!(allowed.contains(&"meta".to_string()));
    }

    #[test]
    fn aggregate_merges_alias_counts() {
        let merged = aggregate_tag_counts(&[("工作流".into(), 3), ("workflow".into(), 2)]);
        assert_eq!(merged, vec![("workflow".to_string(), 5)]);
    }

    #[test]
    fn parse_tags_input_json_array() {
        let tags = parse_tags_input(Some(r#"["meta", "工作流"]"#)).unwrap();
        assert_eq!(tags, vec!["meta", "workflow"]);
    }
}
