use std::env;

/// 阿里云短信（Dysmsapi SendSms）配置。
#[derive(Debug, Clone)]
pub struct AliyunSmsConfig {
    pub access_key_id: String,
    pub access_key_secret: String,
    pub sign_name: String,
    pub template_code: String,
    /// 默认 `cn-hangzhou`
    pub region_id: String,
}

impl AliyunSmsConfig {
    pub fn from_env() -> Option<Self> {
        let access_key_id = env::var("PROMPTSTDIO_ALIYUN_ACCESS_KEY_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let access_key_secret = env::var("PROMPTSTDIO_ALIYUN_ACCESS_KEY_SECRET")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let sign_name = env::var("PROMPTSTDIO_ALIYUN_SMS_SIGN_NAME")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let template_code = env::var("PROMPTSTDIO_ALIYUN_SMS_TEMPLATE_CODE")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let region_id =
            env::var("PROMPTSTDIO_ALIYUN_SMS_REGION").unwrap_or_else(|_| "cn-hangzhou".into());
        Some(Self {
            access_key_id: access_key_id.trim().to_string(),
            access_key_secret: access_key_secret.trim().to_string(),
            sign_name: sign_name.trim().to_string(),
            template_code: template_code.trim().to_string(),
            region_id: region_id.trim().to_string(),
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.access_key_id.is_empty() {
            return Err("PROMPTSTDIO_ALIYUN_ACCESS_KEY_ID 未配置".into());
        }
        if self.access_key_secret.is_empty() {
            return Err("PROMPTSTDIO_ALIYUN_ACCESS_KEY_SECRET 未配置".into());
        }
        if self.sign_name.is_empty() {
            return Err("PROMPTSTDIO_ALIYUN_SMS_SIGN_NAME 未配置（短信签名）".into());
        }
        if self.template_code.is_empty() {
            return Err("PROMPTSTDIO_ALIYUN_SMS_TEMPLATE_CODE 未配置（短信模板 CODE）".into());
        }
        Ok(())
    }
}
