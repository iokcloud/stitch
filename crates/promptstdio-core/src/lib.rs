//! 共享内核：配置、错误、常量、Schema 校验入口。

pub mod aliyun_sms;
pub mod config;
pub mod error;
pub mod explore_access;
pub mod github_oauth;
pub mod limits;
pub mod local_dev;
pub mod messages;
#[cfg(feature = "server")]
pub mod rate_limit;
#[cfg(feature = "server")]
pub mod schema;
pub mod session;
pub mod smtp;
pub mod tags;
pub mod token_hash;
#[cfg(feature = "server")]
pub mod tracing_init;

pub use aliyun_sms::AliyunSmsConfig;
pub use config::AppConfig;
pub use error::{AppError, AppResult};
pub use explore_access::{
    ACCESS_LEVEL_MEMBER_ONLY, ACCESS_LEVEL_PRIVATE, ACCESS_LEVEL_PUBLIC, TAG_IDE_SKILL,
    is_member_only, normalize_prompt_access_level, normalize_suite_access_level,
};
pub use github_oauth::GitHubOAuthConfig;
pub use local_dev::{
    LOCAL_DEV_EMAIL, LOCAL_DEV_NAME, LOCAL_DEV_PASSWORD, PAYTEST_DEV_EMAIL, PAYTEST_DEV_NAME,
};
#[cfg(feature = "server")]
pub use rate_limit::SlidingWindowRateLimiter;
pub use session::SESSION_TOKEN_COOKIE;
pub use smtp::SmtpConfig;
pub use tags::{
    TagLocale, aggregate_tag_counts, catalog_keys, filter_variants, harvest_allowed_keys,
    max_tag_count, normalize_tag, normalize_tags, normalize_tags_from_json, parse_csv_tags,
    parse_tags_input, tag_label, tag_labels, tags_display_labels, tags_match_filter,
};
pub use token_hash::TokenHasher;
#[cfg(feature = "server")]
pub use tracing_init::init_tracing;
