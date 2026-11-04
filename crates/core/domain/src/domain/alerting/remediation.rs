// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 修复动作：组织事先定义好、在事件上一键发起的处置，一律经过审批再执行。
//!
//! - HTTP：调用 Jenkins、GitHub Actions、Argo、Rundeck 这类系统；请求头与签名密钥加密保存，请求体是
//!   带事件变量和参数的模板。
//! - Kubernetes：在登记的集群上重启工作负载、扩缩容，或把 Deployment 回滚到上一个版本。
//!
//! 模板里用 `{{incident.id}}`、`{{incident.summary}}`、`{{incident.severity}}`、`{{incident.service}}`、
//! `{{incident.url}}` 和 `{{params.<名字>}}`。填进地址的值做百分号编码，填进 JSON 的值按 JSON 字符串转义。

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::incident::Incident;
pub use super::remediation_template::{Escape, reads_as_json, render_template, template_vars};
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

pub const MAX_REMEDIATION_PARAMETERS: usize = 10;
pub const MAX_REMEDIATION_TEXT_CHARS: usize = 2_000;
pub const MAX_REMEDIATION_BODY_CHARS: usize = 16_384;
pub const MAX_VERIFY_AFTER_SECS: u32 = 86_400;

/// 一个参数的取值最多多少字。
pub const MAX_PARAMETER_VALUE_CHARS: usize = 500;

/// 风险等级：L2 一人批准，L3 两人批准。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemediationRisk {
    #[default]
    L2,
    L3,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterKind {
    #[default]
    Text,
    Number,
    Select,
}

/// 发起时要填的一个参数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemediationParameter {
    /// 模板里写 `{{params.<key>}}`。
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub kind: ParameterKind,
    /// 选项（`select`）。
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpVerb {
    Get,
    #[default]
    Post,
    Put,
    Patch,
}

impl HttpVerb {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
        }
    }
}

/// 调一个 HTTP 地址。请求头与签名密钥是秘密，存的时候加密，读出来给页面时不带。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRemediation {
    #[serde(default)]
    pub method: HttpVerb,
    /// 地址模板。
    pub url: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// 请求体模板，是 JSON（占位符可以不带引号，当数字用）；空着不带请求体。
    #[serde(default)]
    pub body: String,
    /// 设了就带 `X-MoleSignal-Timestamp` 与 `X-MoleSignal-Sign`，同 Webhook。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_secret: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KubernetesOperation {
    /// 滚动重启（同 `kubectl rollout restart`）。
    Restart,
    /// 改副本数。
    Scale,
    /// Deployment 回到上一个版本（同 `kubectl rollout undo`）。
    Rollback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkloadKind {
    Deployment,
    StatefulSet,
    DaemonSet,
}

impl WorkloadKind {
    /// API 路径里的复数名。
    pub const fn resource(self) -> &'static str {
        match self {
            Self::Deployment => "deployments",
            Self::StatefulSet => "statefulsets",
            Self::DaemonSet => "daemonsets",
        }
    }
}

/// 在一个集群的一个工作负载上做的事。命名空间、名字与副本数都可以是模板。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesRemediation {
    pub cluster_id: Id,
    pub operation: KubernetesOperation,
    pub namespace: String,
    pub workload_kind: WorkloadKind,
    pub workload_name: String,
    /// 扩缩容的副本数，如 `{{params.replicas}}`。
    #[serde(default)]
    pub replicas: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RemediationTarget {
    Http(HttpRemediation),
    Kubernetes(KubernetesRemediation),
}

/// 执行后怎样验证。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyBy {
    /// 事件恢复了（解决或关闭）。
    #[default]
    IncidentResolved,
    /// 事件的规则回到健康。
    RuleHealthy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemediationAction {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub target: RemediationTarget,
    #[serde(default)]
    pub parameters: Vec<RemediationParameter>,
    /// 适用的服务；和团队都空着时所有事件都能用。
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default)]
    pub team_ids: Vec<Id>,
    #[serde(default)]
    pub risk: RemediationRisk,
    /// 执行后多少秒验证；0 不验证。
    #[serde(default)]
    pub verify_after_secs: u32,
    #[serde(default)]
    pub verify_by: VerifyBy,
    pub enabled: bool,
    /// 页面上看哪些秘密已经设了：`signing_secret`、`header:<名字>`。秘密本身不出库。
    #[serde(default)]
    pub secrets_set: Vec<String>,
    pub created_by: Option<Id>,
    pub updated_by: Option<Id>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl RemediationAction {
    pub fn validate(&mut self) -> Result<()> {
        self.name = self.name.trim().to_string();

        if self.name.is_empty() || self.name.chars().count() > 120 {
            return Err(Error::invalid(
                "a remediation action's name has 1 to 120 characters",
            ));
        }

        if self.description.chars().count() > MAX_REMEDIATION_TEXT_CHARS {
            return Err(Error::invalid(format!(
                "a description is at most {MAX_REMEDIATION_TEXT_CHARS} characters"
            )));
        }

        if self.parameters.len() > MAX_REMEDIATION_PARAMETERS {
            return Err(Error::invalid(format!(
                "a remediation action takes at most {MAX_REMEDIATION_PARAMETERS} parameters"
            )));
        }

        let mut keys = Vec::new();

        for parameter in &mut self.parameters {
            parameter.key = parameter.key.trim().to_string();
            parameter.label = parameter.label.trim().to_string();

            let fine = !parameter.key.is_empty()
                && parameter.key.len() <= 40
                && parameter
                    .key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_');

            if !fine {
                return Err(Error::invalid(
                    "a parameter's key has 1 to 40 letters, digits or underscores",
                ));
            }

            if keys.contains(&parameter.key) {
                return Err(Error::invalid(format!(
                    "parameter `{}` is named twice",
                    parameter.key
                )));
            }

            if parameter.kind == ParameterKind::Select && parameter.options.is_empty() {
                return Err(Error::invalid(format!(
                    "parameter `{}` chooses from options; name at least one",
                    parameter.key
                )));
            }

            if parameter.label.is_empty() {
                parameter.label = parameter.key.clone();
            }

            keys.push(parameter.key.clone());
        }

        if self.verify_after_secs > MAX_VERIFY_AFTER_SECS {
            return Err(Error::invalid(format!(
                "verify_after_secs is at most {MAX_VERIFY_AFTER_SECS}"
            )));
        }

        match &mut self.target {
            RemediationTarget::Http(http) => {
                http.url = http.url.trim().to_string();

                let lower = http.url.to_ascii_lowercase();

                if !(lower.starts_with("https://") || lower.starts_with("http://")) {
                    return Err(Error::invalid(
                        "the address starts with http:// or https://",
                    ));
                }

                if http.body.chars().count() > MAX_REMEDIATION_BODY_CHARS {
                    return Err(Error::invalid(format!(
                        "the request body is at most {MAX_REMEDIATION_BODY_CHARS} characters"
                    )));
                }

                if !http.body.trim().is_empty() && !reads_as_json(&http.body) {
                    return Err(Error::invalid(
                        "the request body is JSON: with its placeholders filled in, it does not read as JSON",
                    ));
                }

                if http
                    .headers
                    .keys()
                    .any(|name| name.trim().is_empty() || name.contains([':', '\r', '\n']))
                {
                    return Err(Error::invalid("header names cannot be empty or hold `:`"));
                }
            }
            RemediationTarget::Kubernetes(kubernetes) => {
                kubernetes.namespace = kubernetes.namespace.trim().to_string();
                kubernetes.workload_name = kubernetes.workload_name.trim().to_string();

                if kubernetes.namespace.is_empty() || kubernetes.workload_name.is_empty() {
                    return Err(Error::invalid(
                        "a Kubernetes action names the namespace and the workload",
                    ));
                }

                if kubernetes.operation == KubernetesOperation::Rollback
                    && kubernetes.workload_kind != WorkloadKind::Deployment
                {
                    return Err(Error::invalid("only a Deployment is rolled back"));
                }

                if kubernetes.operation == KubernetesOperation::Scale
                    && kubernetes
                        .replicas
                        .as_deref()
                        .is_none_or(|replicas| replicas.trim().is_empty())
                {
                    return Err(Error::invalid("scaling names the number of replicas"));
                }

                if kubernetes.operation == KubernetesOperation::Scale
                    && kubernetes.workload_kind == WorkloadKind::DaemonSet
                {
                    return Err(Error::invalid("a DaemonSet has no replica count to scale"));
                }
            }
        }

        Ok(())
    }

    /// 这个事件能不能用它：服务对得上、团队对得上，或两样都没限定。
    pub fn applies_to(&self, incident: &Incident) -> bool {
        if self.services.is_empty() && self.team_ids.is_empty() {
            return true;
        }

        let service_fits = self.services.iter().any(|service| {
            incident
                .affected_services
                .iter()
                .chain(incident.labels.get("service"))
                .any(|own| own.eq_ignore_ascii_case(service.trim()))
        });

        let team_fits = incident
            .team_id
            .as_ref()
            .is_some_and(|team| self.team_ids.contains(team));

        service_fits || team_fits
    }

    /// 核对发起时填的参数：必填的都有，数字是数字，选项在里面，没有多出来的；没填的用默认值。
    pub fn inputs(&self, given: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>> {
        if let Some(extra) = given.keys().find(|key| {
            !self
                .parameters
                .iter()
                .any(|parameter| &parameter.key == *key)
        }) {
            return Err(Error::invalid(format!(
                "`{extra}` is not a parameter of this action"
            )));
        }

        let mut inputs = BTreeMap::new();

        for parameter in &self.parameters {
            let value = given
                .get(&parameter.key)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .or_else(|| parameter.default.clone());

            let Some(value) = value else {
                if parameter.required {
                    return Err(Error::invalid(format!("`{}` is required", parameter.label)));
                }

                continue;
            };

            if value.chars().count() > MAX_PARAMETER_VALUE_CHARS {
                return Err(Error::invalid(format!(
                    "`{}` is at most {MAX_PARAMETER_VALUE_CHARS} characters",
                    parameter.label
                )));
            }

            if parameter.kind == ParameterKind::Number && value.parse::<f64>().is_err() {
                return Err(Error::invalid(format!("`{}` is a number", parameter.label)));
            }

            if parameter.kind == ParameterKind::Select && !parameter.options.contains(&value) {
                return Err(Error::invalid(format!(
                    "`{}` is one of {}",
                    parameter.label,
                    parameter.options.join(", ")
                )));
            }

            inputs.insert(parameter.key.clone(), value);
        }

        Ok(inputs)
    }
}

/// 登记的 Kubernetes 集群。Token 是秘密，读出来给页面时不带。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesCluster {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    /// API Server 地址，如 `https://10.0.0.1:6443`。
    pub api_server: String,
    /// 校验 API Server 证书用的 CA（PEM）；空着用系统信任的根证书。
    #[serde(default)]
    pub ca_cert: String,
    /// ServiceAccount Token。
    #[serde(default, skip_serializing)]
    pub token: String,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl KubernetesCluster {
    pub fn validate(&mut self) -> Result<()> {
        self.name = self.name.trim().to_string();
        self.api_server = self.api_server.trim().trim_end_matches('/').to_string();

        if self.name.is_empty() || self.name.chars().count() > 120 {
            return Err(Error::invalid("a cluster's name has 1 to 120 characters"));
        }

        if !self.api_server.to_ascii_lowercase().starts_with("https://") {
            return Err(Error::invalid(
                "the API server address starts with https://",
            ));
        }

        let ca = self.ca_cert.trim();

        if !ca.is_empty() && !ca.starts_with("-----BEGIN CERTIFICATE-----") {
            return Err(Error::invalid("the CA is a PEM certificate"));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemediationRunStatus {
    /// 等审批。
    AwaitingApproval,
    Running,
    Succeeded,
    Failed,
    /// 审批被驳回或过期。
    Declined,
    /// 执行成功，等验证。
    Verifying,
    Verified,
    VerificationFailed,
}

impl RemediationRunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingApproval => "awaiting_approval",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Declined => "declined",
            Self::Verifying => "verifying",
            Self::Verified => "verified",
            Self::VerificationFailed => "verification_failed",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "running" => Self::Running,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "declined" => Self::Declined,
            "verifying" => Self::Verifying,
            "verified" => Self::Verified,
            "verification_failed" => Self::VerificationFailed,
            _ => Self::AwaitingApproval,
        }
    }
}

/// 在一个事件上发起的一次修复动作。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemediationRun {
    pub id: Id,
    pub org_id: Id,
    pub incident_id: Id,
    pub action_id: Id,
    pub action_name: String,
    /// 它的审批请求。
    pub approval_id: Id,
    pub inputs: BTreeMap<String, String>,
    pub status: RemediationRunStatus,
    /// 执行的结果：HTTP 状态码与响应开头，或 Kubernetes 做了什么。
    #[serde(default)]
    pub output: String,
    pub verify_at: Option<TimestampMicros>,
    #[serde(default)]
    pub verification: String,
    pub requested_by: Id,
    pub executed_by: Option<Id>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait RemediationRepository: Send + Sync {
    async fn list_actions(&self, org_id: &Id) -> Result<Vec<RemediationAction>>;
    /// 带着秘密（请求头、签名密钥）读出，执行时用。
    async fn get_action(&self, org_id: &Id, id: &Id) -> Result<Option<RemediationAction>>;
    async fn save_action(&self, action: RemediationAction) -> Result<RemediationAction>;
    async fn delete_action(&self, org_id: &Id, id: &Id) -> Result<bool>;

    async fn list_clusters(&self, org_id: &Id) -> Result<Vec<KubernetesCluster>>;
    /// 带着 Token 读出。
    async fn get_cluster(&self, org_id: &Id, id: &Id) -> Result<Option<KubernetesCluster>>;
    async fn save_cluster(&self, cluster: KubernetesCluster) -> Result<KubernetesCluster>;
    async fn delete_cluster(&self, org_id: &Id, id: &Id) -> Result<bool>;

    async fn create_run(&self, run: RemediationRun) -> Result<RemediationRun>;
    async fn update_run(&self, run: RemediationRun) -> Result<RemediationRun>;
    async fn find_run_by_approval(
        &self,
        org_id: &Id,
        approval_id: &Id,
    ) -> Result<Option<RemediationRun>>;
    async fn list_runs_for_incident(
        &self,
        org_id: &Id,
        incident_id: &Id,
    ) -> Result<Vec<RemediationRun>>;
    /// 到了验证时间、还在等验证的。
    async fn due_verifications(
        &self,
        now: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<RemediationRun>>;
    /// 还在等审批的（看审批有没有被驳回、过期）。
    async fn awaiting_approval(&self, limit: i64) -> Result<Vec<RemediationRun>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(target: RemediationTarget) -> RemediationAction {
        RemediationAction {
            id: Id::from_string("act-1"),
            org_id: Id::from_string("org-1"),
            name: "Restart checkout".into(),
            description: String::new(),
            target,
            parameters: vec![
                RemediationParameter {
                    key: "replicas".into(),
                    label: "Replicas".into(),
                    kind: ParameterKind::Number,
                    options: Vec::new(),
                    required: true,
                    default: None,
                },
                RemediationParameter {
                    key: "region".into(),
                    label: "Region".into(),
                    kind: ParameterKind::Select,
                    options: vec!["eu".into(), "us".into()],
                    required: false,
                    default: Some("eu".into()),
                },
            ],
            services: vec!["checkout".into()],
            team_ids: Vec::new(),
            risk: RemediationRisk::L2,
            verify_after_secs: 300,
            verify_by: VerifyBy::IncidentResolved,
            enabled: true,
            secrets_set: Vec::new(),
            created_by: None,
            updated_by: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn scale() -> RemediationTarget {
        RemediationTarget::Kubernetes(KubernetesRemediation {
            cluster_id: Id::from_string("k8s-1"),
            operation: KubernetesOperation::Scale,
            namespace: "shop".into(),
            workload_kind: WorkloadKind::Deployment,
            workload_name: "checkout".into(),
            replicas: Some("{{params.replicas}}".into()),
        })
    }

    #[test]
    fn inputs_follow_the_parameters() {
        let action = action(scale());
        let given = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect::<BTreeMap<_, _>>()
        };

        let inputs = action.inputs(&given(&[("replicas", "6")])).unwrap();
        assert_eq!(inputs.get("region").map(String::as_str), Some("eu"));

        assert!(action.inputs(&given(&[])).is_err());
        assert!(action.inputs(&given(&[("replicas", "six")])).is_err());
        assert!(
            action
                .inputs(&given(&[("replicas", "6"), ("region", "ap")]))
                .is_err()
        );
        assert!(
            action
                .inputs(&given(&[("replicas", "6"), ("zone", "a")]))
                .is_err()
        );
    }

    #[test]
    fn a_body_reads_as_json_with_its_placeholders_filled_in() {
        assert!(reads_as_json(
            r#"{"service": "{{incident.service}}", "reason": "{{params.reason}}"}"#
        ));
        assert!(reads_as_json(r#"{"replicas": {{params.replicas}}}"#));
        assert!(!reads_as_json("restart {{params.job}}"));
        assert!(!reads_as_json(r#"{"job": "{{params.job}}""#));
        assert!(!reads_as_json(r#"{"job": {{params.job}"#));
    }

    #[test]
    fn templates_escape_for_where_the_value_goes() {
        let vars = BTreeMap::from([
            (
                "incident.summary".to_string(),
                "p99 \"slow\" & rising".to_string(),
            ),
            ("params.region".to_string(), "eu west".to_string()),
        ]);

        assert_eq!(
            render_template(r#"{"text":"{{incident.summary}}"}"#, &vars, Escape::Json),
            r#"{"text":"p99 \"slow\" & rising"}"#
        );
        assert_eq!(
            render_template(
                "https://ci/job?region={{ params.region }}&x={{missing}}",
                &vars,
                Escape::Url
            ),
            "https://ci/job?region=eu%20west&x="
        );
        assert_eq!(
            render_template("no {{ end", &vars, Escape::None),
            "no {{ end"
        );
    }

    #[test]
    fn kubernetes_actions_say_what_they_touch() {
        assert!(action(scale()).validate().is_ok());

        let rollback_statefulset = RemediationTarget::Kubernetes(KubernetesRemediation {
            cluster_id: Id::from_string("k8s-1"),
            operation: KubernetesOperation::Rollback,
            namespace: "shop".into(),
            workload_kind: WorkloadKind::StatefulSet,
            workload_name: "db".into(),
            replicas: None,
        });
        assert!(action(rollback_statefulset).validate().is_err());

        let http = RemediationTarget::Http(HttpRemediation {
            method: HttpVerb::Post,
            url: "ftp://ci".into(),
            headers: BTreeMap::new(),
            body: String::new(),
            signing_secret: None,
        });
        assert!(action(http).validate().is_err());
    }
}
