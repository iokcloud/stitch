//! 本地开发默认账号（与 Laravel `promptstdio:local-bootstrap` 一致）。

pub const LOCAL_DEV_EMAIL: &str = "local@promptstdio.test";
pub const LOCAL_DEV_NAME: &str = "Local Dev";
pub const LOCAL_DEV_PASSWORD: &str = "password";

/// 非会员专用：支付 UI 全流程（收银台 QR · dev/simulate · 跳转）。
pub const PAYTEST_DEV_EMAIL: &str = "paytest@promptstdio.test";
pub const PAYTEST_DEV_NAME: &str = "Pay Test";
