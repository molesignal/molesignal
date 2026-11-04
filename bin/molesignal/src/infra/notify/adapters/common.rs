// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 钉钉、企业微信、Teams 与 Google Chat 连接器共用的部分：配置校验、HTTP 客户端、投递结果、
//! 应用的访问令牌缓存，以及把消息排成各平台认得的样子。

use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use aws_lc_rs::digest;
use url::Url;

use crate::{
    domain::notify::connector::{ConnectorDeliveryResult, NotifyMessage},
    shared::{Error, Result},
};

pub(super) fn default_timeout_secs() -> u64 {
    10
}

pub(crate) fn validate_timeout(timeout_secs: u64, connector: &str) -> Result<()> {
    if (1..=60).contains(&timeout_secs) {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "{connector} timeout_secs must be between 1 and 60"
        )))
    }
}

pub(crate) fn validate_http_url(value: &str, field: &str) -> Result<()> {
    let parsed = Url::parse(value.trim())
        .map_err(|_| Error::invalid(format!("{field} must be an absolute URL")))?;

    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(Error::invalid(format!("{field} must use http or https")));
    }

    Ok(())
}

pub(crate) fn require(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(Error::invalid(format!("{field} cannot be empty")))
    } else {
        Ok(())
    }
}

/// 带查询参数的地址（参数按 URL 编码）。
pub(crate) fn with_query(base: &str, pairs: &[(&str, &str)]) -> Result<Url> {
    Url::parse_with_params(base, pairs)
        .map_err(|_| Error::invalid(format!("{base} is not an absolute URL")))
}

pub(crate) fn client(timeout_secs: u64, connector: &str) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|_| Error::internal(format!("{connector} HTTP client could not be initialized")))
}

/// 平台的错误说明：空的写成 `unknown_error`，太长的截断。
pub(crate) fn provider_message(message: &str) -> String {
    let message = message.trim();

    if message.is_empty() {
        "unknown_error".into()
    } else {
        truncate_chars(message, 300)
    }
}

pub(crate) fn delivered(
    provider_message_id: Option<String>,
    started: Instant,
) -> ConnectorDeliveryResult {
    ConnectorDeliveryResult {
        provider_message_id,
        delivered: true,
        latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        metadata: BTreeMap::new(),
    }
}

/// 消息的正文：有 Markdown 用 Markdown，否则用纯文本。
pub(crate) fn body(message: &NotifyMessage) -> &str {
    message
        .markdown
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&message.text)
}

pub(crate) fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }

    let mut out = value
        .chars()
        .take(max.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
}

/// 按 UTF-8 字节截断（企业微信按字节限制长度），不切断字符。
pub(crate) fn truncate_bytes(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }

    let ellipsis = "…";
    let mut end = max.saturating_sub(ellipsis.len());

    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }

    format!("{}{ellipsis}", &value[..end])
}

/// 标准 Markdown 换成 Google Chat 认得的格式：`*粗体*`、`<链接|文字>`，标题写成粗体行。
pub(crate) fn google_chat_text(markdown: &str) -> String {
    let mut lines = Vec::new();

    for line in markdown.lines() {
        let trimmed = line.trim_start();
        let heading = trimmed.trim_start_matches('#');

        let line = if trimmed.starts_with('#') && heading.starts_with(' ') {
            format!("*{}*", heading.trim())
        } else {
            line.replace("**", "*").replace("__", "_")
        };

        lines.push(links(&line));
    }

    lines.join("\n")
}

/// `[文字](链接)` 换成 `<链接|文字>`。
fn links(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;

    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](").map(|at| open + at) else {
            break;
        };
        let Some(end) = rest[close..].find(')').map(|at| close + at) else {
            break;
        };

        let text = &rest[open + 1..close];
        let url = &rest[close + 2..end];

        out.push_str(&rest[..open]);

        if url.starts_with("http://") || url.starts_with("https://") {
            out.push_str(&format!("<{url}|{text}>"));
        } else {
            out.push_str(&rest[open..=end]);
        }

        rest = &rest[end + 1..];
    }

    out.push_str(rest);
    out
}

/// 应用的访问令牌：按应用与密钥的摘要缓存到过期前一分钟。
pub(crate) struct TokenCache {
    tokens: Mutex<HashMap<String, (String, Instant)>>,
}

pub(crate) static TOKENS: LazyLock<TokenCache> = LazyLock::new(|| TokenCache {
    tokens: Mutex::new(HashMap::new()),
});

impl TokenCache {
    /// 缓存键：连接器类型、应用与密钥的摘要（换了密钥就换一个令牌）。
    pub(crate) fn key(kind: &str, app: &str, secret: &str) -> String {
        let digest = digest::digest(&digest::SHA256, secret.as_bytes());
        let short = digest
            .as_ref()
            .iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();

        format!("{kind}:{app}:{short}")
    }

    /// 缓存里有没过期的就用，否则取一个新的。`fetch` 返回令牌与它的有效秒数。
    pub(crate) async fn get<F>(&self, key: &str, fetch: F) -> Result<String>
    where
        F: Future<Output = Result<(String, u64)>>,
    {
        if let Some(token) = self
            .tokens
            .lock()
            .ok()
            .and_then(|tokens| tokens.get(key).cloned())
            .filter(|(_, until)| Instant::now() < *until)
            .map(|(token, _)| token)
        {
            return Ok(token);
        }

        let (token, expires_in) = fetch.await?;
        let lifetime = Duration::from_secs(expires_in.saturating_sub(60).max(30));

        if let Ok(mut tokens) = self.tokens.lock() {
            tokens.insert(key.to_string(), (token.clone(), Instant::now() + lifetime));
        }

        Ok(token)
    }

    /// 令牌被平台拒绝时丢掉它，下次重新取。
    pub(crate) fn forget(&self, key: &str) {
        if let Ok(mut tokens) = self.tokens.lock() {
            tokens.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_keeps_characters_whole() {
        assert_eq!(truncate_chars("abcdef", 4), "abc…");
        assert_eq!(truncate_chars("abc", 4), "abc");

        let cut = truncate_bytes("告警告警", 8);
        assert!(cut.len() <= 8);
        assert_eq!(cut, "告…");
    }

    #[test]
    fn markdown_reads_as_google_chat_formatting() {
        let text = google_chat_text(
            "## Checkout errors\n**5xx** rose, see [the trace](https://ms.example/t/1) and [x](rel)",
        );

        assert_eq!(
            text,
            "*Checkout errors*\n*5xx* rose, see <https://ms.example/t/1|the trace> and [x](rel)"
        );
    }

    #[tokio::test]
    async fn a_token_is_fetched_once_until_it_is_forgotten() {
        let cache = TokenCache {
            tokens: Mutex::new(HashMap::new()),
        };
        let key = TokenCache::key("dingtalk_app", "app", "secret");

        let first = cache
            .get(&key, async { Ok(("t1".to_string(), 7_200)) })
            .await
            .unwrap();
        let cached = cache
            .get(&key, async { Ok(("t2".to_string(), 7_200)) })
            .await
            .unwrap();
        assert_eq!((first.as_str(), cached.as_str()), ("t1", "t1"));

        cache.forget(&key);
        let fresh = cache
            .get(&key, async { Ok(("t3".to_string(), 7_200)) })
            .await
            .unwrap();
        assert_eq!(fresh, "t3");
        assert_ne!(key, TokenCache::key("dingtalk_app", "app", "other"));
    }
}
