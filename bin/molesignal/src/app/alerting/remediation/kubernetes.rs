// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Kubernetes 修复动作，直接调 API Server（apps/v1），用登记的 ServiceAccount Token：
//!
//! - 重启：给 Pod 模板写上 `kubectl.kubernetes.io/restartedAt`，同 `kubectl rollout restart`；
//! - 扩缩容：改 `/scale` 子资源的副本数；
//! - 回滚：找到这个 Deployment 名下版本号比当前小的最新 ReplicaSet，把它的 Pod 模板换回去（去掉
//!   `pod-template-hash`），同 `kubectl rollout undo`。
//!
//! 只连 API Server 解析出的地址（不能是云厂商元数据地址），不跟随跳转；设了 CA 时只认它签的证书。

use std::{collections::BTreeMap, net::SocketAddr, time::Duration};

use chrono::Utc;
use reqwest::{Method, header::CONTENT_TYPE, redirect::Policy};
use serde_json::{Value, json};

use crate::{
    domain::alerting::remediation::{
        Escape, KubernetesCluster, KubernetesOperation, KubernetesRemediation, render_template,
    },
    infra::connectors::config::http::is_metadata_address,
    shared::{Error, Result},
};

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REPLICAS: u32 = 1_000;
const REVISION: &str = "deployment.kubernetes.io/revision";

/// 做一次，返回做了什么。
pub async fn run(
    cluster: &KubernetesCluster,
    action: &KubernetesRemediation,
    vars: &BTreeMap<String, String>,
) -> Result<String> {
    let namespace = name_of(
        &render_template(&action.namespace, vars, Escape::None),
        "namespace",
    )?;
    let workload = name_of(
        &render_template(&action.workload_name, vars, Escape::None),
        "workload",
    )?;
    let api = Api::new(cluster).await?;
    let path = format!(
        "/apis/apps/v1/namespaces/{namespace}/{}/{workload}",
        action.workload_kind.resource()
    );
    let what = format!("{:?} {namespace}/{workload}", action.workload_kind);

    match action.operation {
        KubernetesOperation::Restart => {
            let patch = json!({ "spec": { "template": { "metadata": { "annotations": {
                "kubectl.kubernetes.io/restartedAt": Utc::now().to_rfc3339(),
            } } } } });

            api.send(
                Method::PATCH,
                &path,
                Some(("application/strategic-merge-patch+json", patch)),
            )
            .await?;

            Ok(format!("restarted {what}"))
        }
        KubernetesOperation::Scale => {
            let text = render_template(
                action.replicas.as_deref().unwrap_or_default(),
                vars,
                Escape::None,
            );
            let replicas = text
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|replicas| *replicas <= MAX_REPLICAS)
                .ok_or_else(|| {
                    Error::invalid(format!(
                        "`{text}` is not a replica count from 0 to {MAX_REPLICAS}"
                    ))
                })?;

            api.send(
                Method::PATCH,
                &format!("{path}/scale"),
                Some((
                    "application/merge-patch+json",
                    json!({ "spec": { "replicas": replicas } }),
                )),
            )
            .await?;

            Ok(format!("scaled {what} to {replicas} replicas"))
        }
        KubernetesOperation::Rollback => {
            let deployment = api.send(Method::GET, &path, None).await?;
            let selector = label_selector(&deployment)?;
            let sets = api
                .send(
                    Method::GET,
                    &format!(
                        "/apis/apps/v1/namespaces/{namespace}/replicasets?labelSelector={}",
                        percent(&selector)
                    ),
                    None,
                )
                .await?;
            let (revision, template) = previous_template(&deployment, &sets)?;

            api.send(
                Method::PATCH,
                &path,
                Some((
                    "application/json-patch+json",
                    json!([{ "op": "replace", "path": "/spec/template", "value": template }]),
                )),
            )
            .await?;

            Ok(format!("rolled {what} back to revision {revision}"))
        }
    }
}

struct Api {
    base: String,
    token: String,
    client: reqwest::Client,
}

impl Api {
    async fn new(cluster: &KubernetesCluster) -> Result<Self> {
        let url = url::Url::parse(&cluster.api_server)
            .map_err(|error| Error::invalid(format!("the API server address: {error}")))?;

        let mut builder = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .connect_timeout(Duration::from_secs(5))
            .redirect(Policy::none());

        let ca = cluster.ca_cert.trim();

        if !ca.is_empty() {
            let certificate = reqwest::Certificate::from_pem(ca.as_bytes())
                .map_err(|error| Error::invalid(format!("the cluster CA: {error}")))?;
            builder = builder.add_root_certificate(certificate);
        }

        let host = url
            .host_str()
            .ok_or_else(|| Error::invalid("the API server address has no host"))?
            .trim_matches(['[', ']'])
            .to_string();
        let port = url.port_or_known_default().unwrap_or(443);

        let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|error| {
                Error::unavailable(format!("`{host}` could not be resolved: {error}"))
            })?
            .collect();

        if addresses.is_empty() {
            return Err(Error::unavailable(format!(
                "`{host}` resolves to no address"
            )));
        }

        if let Some(address) = addresses
            .iter()
            .find(|address| is_metadata_address(address.ip()))
        {
            return Err(Error::forbidden(format!(
                "`{host}` resolves to `{}`, where cloud metadata services answer",
                address.ip()
            )));
        }

        if matches!(url.host(), Some(url::Host::Domain(_))) {
            builder = builder.resolve_to_addrs(&host, &addresses);
        }

        Ok(Self {
            base: cluster.api_server.trim_end_matches('/').to_string(),
            token: cluster.token.clone(),
            client: builder
                .build()
                .map_err(|error| Error::internal(format!("kubernetes client: {error}")))?,
        })
    }

    async fn send(&self, method: Method, path: &str, body: Option<(&str, Value)>) -> Result<Value> {
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .header("Accept", "application/json");

        if let Some((content_type, body)) = body {
            request = request
                .header(CONTENT_TYPE, content_type)
                .body(body.to_string());
        }

        let response = request.send().await.map_err(|error| {
            Error::unavailable(format!("the API server could not be reached: {error}"))
        })?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);

        if !status.is_success() {
            let message = parsed["message"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| text.chars().take(300).collect());

            return Err(Error::unavailable(format!(
                "the API server answered {}: {message}",
                status.as_u16()
            )));
        }

        Ok(parsed)
    }

    /// 凭证与地址能用：读得到 API Server 的版本。
    async fn version(&self) -> Result<String> {
        let version = self.send(Method::GET, "/version", None).await?;

        Ok(version["gitVersion"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }
}

/// 集群连得上：返回 Kubernetes 的版本。
pub async fn check(cluster: &KubernetesCluster) -> Result<String> {
    Api::new(cluster).await?.version().await
}

/// 命名空间与工作负载的名字：小写字母、数字、`-` 和 `.`，最长 253。
fn name_of(text: &str, what: &str) -> Result<String> {
    let name = text.trim();
    let fine = !name.is_empty()
        && name.len() <= 253
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.');

    if fine {
        Ok(name.to_string())
    } else {
        Err(Error::invalid(format!(
            "`{name}` is not a Kubernetes {what} name"
        )))
    }
}

fn label_selector(deployment: &Value) -> Result<String> {
    let labels = deployment["spec"]["selector"]["matchLabels"]
        .as_object()
        .filter(|labels| !labels.is_empty())
        .ok_or_else(|| Error::invalid("the Deployment selects its pods by no labels"))?;

    Ok(labels
        .iter()
        .map(|(key, value)| format!("{key}={}", value.as_str().unwrap_or_default()))
        .collect::<Vec<_>>()
        .join(","))
}

/// 这个 Deployment 名下版本号比当前小的最新一个 ReplicaSet：它的版本号与 Pod 模板（去掉
/// `pod-template-hash`）。
fn previous_template(deployment: &Value, sets: &Value) -> Result<(u64, Value)> {
    let uid = deployment["metadata"]["uid"].as_str().unwrap_or_default();
    let current = revision_of(deployment).unwrap_or(u64::MAX);

    let previous = sets["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|set| {
            set["metadata"]["ownerReferences"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|owner| owner["uid"].as_str() == Some(uid))
        })
        .filter_map(|set| revision_of(set).map(|revision| (revision, set)))
        .filter(|(revision, _)| *revision < current)
        .max_by_key(|(revision, _)| *revision)
        .ok_or_else(|| Error::invalid("the Deployment has no earlier revision to roll back to"))?;

    let mut template = previous.1["spec"]["template"].clone();

    if let Some(labels) = template["metadata"]["labels"].as_object_mut() {
        labels.remove("pod-template-hash");
    }

    Ok((previous.0, template))
}

fn revision_of(object: &Value) -> Option<u64> {
    object["metadata"]["annotations"][REVISION]
        .as_str()
        .and_then(|revision| revision.parse().ok())
}

fn percent(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(revision: &str, owner: &str, image: &str) -> Value {
        json!({
            "metadata": {
                "annotations": { REVISION: revision },
                "ownerReferences": [{ "uid": owner }],
            },
            "spec": { "template": {
                "metadata": { "labels": { "app": "checkout", "pod-template-hash": "abc" } },
                "spec": { "containers": [{ "name": "app", "image": image }] },
            } },
        })
    }

    #[test]
    fn rollback_takes_the_latest_earlier_revision_of_this_deployment() {
        let deployment = json!({
            "metadata": { "uid": "dep-1", "annotations": { REVISION: "5" } },
            "spec": { "selector": { "matchLabels": { "app": "checkout" } } },
        });
        let sets = json!({ "items": [
            set("3", "dep-1", "v3"),
            set("4", "dep-1", "v4"),
            set("5", "dep-1", "v5"),
            set("4", "other", "x"),
        ]});

        let (revision, template) = previous_template(&deployment, &sets).unwrap();
        assert_eq!(revision, 4);
        assert_eq!(template["spec"]["containers"][0]["image"], "v4");
        assert!(
            template["metadata"]["labels"]
                .get("pod-template-hash")
                .is_none()
        );
        assert_eq!(label_selector(&deployment).unwrap(), "app=checkout");

        let only = json!({ "items": [set("5", "dep-1", "v5")] });
        assert!(previous_template(&deployment, &only).is_err());
    }

    #[test]
    fn names_are_kubernetes_names() {
        assert_eq!(name_of(" shop-prod ", "namespace").unwrap(), "shop-prod");
        assert!(name_of("Shop", "namespace").is_err());
        assert!(name_of("a/b", "workload").is_err());
        assert!(name_of("", "workload").is_err());
    }
}
