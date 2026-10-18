// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 工具调用的证据：落在 assistant 消息的 `evidence_json` 里，大的原始结果放对象存储。

use object_store::{ObjectStoreExt, PutPayload, path::Path as ObjPath};
use serde_json::{Value, json};

use super::CHAT_OBJECT_LAYOUT_VERSION;
use crate::{api::AppState, shared::ids::Id};

/// 单条 tool 原始结果内联上限（字节）；超此值 spill 到对象存储，evidence 只留 object_key + 摘要。
const INLINE_TOOL_RESULT_LIMIT: usize = 16 * 1024;

/// evidence 摘要文本上限。
const EVIDENCE_SUMMARY_CAP: usize = 500;

/// 构造一条 tool evidence；result 过大时 spill 原始 JSON 到对象存储，evidence 只留 object_key。
#[allow(clippy::too_many_arguments)]
pub(super) async fn build_evidence(
    state: &AppState,
    org_id: &Id,
    chat_id: &Id,
    message_id: &Id,
    tool_call_id: &str,
    name: &str,
    args: Value,
    result_json: &str,
    is_error: bool,
) -> Value {
    let (row_count, scanned_rows, took_ms, summary) = summarize_tool_result(result_json);

    let mut entry = json!({
        "tool_call_id": tool_call_id,
        "tool": name,
        "arguments": args,
        "status": if is_error { "error" } else { "success" },
        "summary": summary,
    });

    if let Some(v) = row_count {
        entry["row_count"] = json!(v);
    }

    if let Some(v) = scanned_rows {
        entry["scanned_rows"] = json!(v);
    }

    if let Some(v) = took_ms {
        entry["took_ms"] = json!(v);
    }

    if result_json.len() > INLINE_TOOL_RESULT_LIMIT {
        let object_key = tool_result_object_key(org_id, chat_id, message_id, tool_call_id);

        match ObjPath::parse(&object_key) {
            Ok(path) => {
                match state
                    .storage
                    .object_store
                    .put(&path, PutPayload::from(result_json.as_bytes().to_vec()))
                    .await
                {
                    Ok(_) => {
                        entry["object_key"] = json!(object_key);
                        entry["spilled_bytes"] = json!(result_json.len());
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to spill tool result to object store");
                        entry["spill_error"] = json!(e.to_string());
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "invalid tool spill object path");
            }
        }
    }

    entry
}

fn tool_result_object_key(
    org_id: &Id,
    chat_id: &Id,
    message_id: &Id,
    tool_call_id: &str,
) -> String {
    format!(
        "agent/chat/{CHAT_OBJECT_LAYOUT_VERSION}/{}/{}/tool-results/{}/{}.json",
        org_id.0, chat_id.0, message_id.0, tool_call_id
    )
}

/// 从 tool 结果 JSON（`Vec<ToolContent>` 序列化）提取 row/scanned/took + 摘要文本。
fn summarize_tool_result(result_json: &str) -> (Option<i64>, Option<i64>, Option<i64>, String) {
    let parsed: Value = serde_json::from_str(result_json).unwrap_or(Value::Null);
    let mut row_count = None;
    let mut scanned = None;
    let mut took = None;
    let mut text_summary = String::new();

    if let Some(arr) = parsed.as_array() {
        for item in arr {
            match item.get("type").and_then(|t| t.as_str()) {
                Some("json") => {
                    let j = item.get("json").cloned().unwrap_or(Value::Null);

                    for key in ["rows", "incidents", "streams", "spans"] {
                        if let Some(a) = j.get(key).and_then(|v| v.as_array()) {
                            row_count = Some(a.len() as i64);
                        }
                    }

                    scanned = j.get("scanned_rows").and_then(|v| v.as_i64()).or(scanned);
                    took = j.get("took_ms").and_then(|v| v.as_i64()).or(took);
                }
                Some("text") => {
                    if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                        text_summary.push_str(t);
                    }
                }
                _ => {}
            }
        }
    }

    let summary = if !text_summary.is_empty() {
        cap_str(&text_summary, EVIDENCE_SUMMARY_CAP)
    } else if let Some(n) = row_count {
        format!("{n} rows")
    } else {
        "ok".to_string()
    };

    (row_count, scanned, took, summary)
}

fn cap_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }

    let truncated: String = s.chars().take(max).collect();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_extracts_row_count() {
        let result =
            r#"[{"type":"json","json":{"rows":[[1],[2],[3]],"scanned_rows":99,"took_ms":7}}]"#;
        let (rows, scanned, took, summary) = summarize_tool_result(result);
        assert_eq!(rows, Some(3));
        assert_eq!(scanned, Some(99));
        assert_eq!(took, Some(7));
        assert_eq!(summary, "3 rows");
    }

    #[test]
    fn summarize_text_result() {
        let result = r#"[{"type":"text","text":"trace not found"}]"#;
        let (_, _, _, summary) = summarize_tool_result(result);
        assert_eq!(summary, "trace not found");
    }

    #[test]
    fn tool_result_object_keys_include_layout_version() {
        let org_id = Id::from_string("org-1");
        let chat_id = Id::from_string("chat-1");
        let message_id = Id::from_string("message-1");
        assert_eq!(
            tool_result_object_key(&org_id, &chat_id, &message_id, "tool-1"),
            "agent/chat/v1/org-1/chat-1/tool-results/message-1/tool-1.json"
        );
    }
}
