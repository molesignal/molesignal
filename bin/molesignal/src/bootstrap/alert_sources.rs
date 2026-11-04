// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警来源的装配：来源、收到的请求与外部事件的仓库，事件评估器，以及联邦里把告警转给接收集群
//! 的 gRPC 转发。

use std::sync::Arc;

use super::{alerting::AlertingRuntime, core::Core, iam::IamRuntime, query::QueryRuntime};
use crate::{
    app::alerting::{
        AlertSources,
        sources::{
            AlertForwarder, AlertSourcesParts, InstanceSettingsCluster, MailboxClient,
            MailboxPoller,
        },
    },
    config::Settings,
    infra::{
        cluster::GrpcAlertForwarder,
        mailbox::ImapMailboxClient,
        persistence::repositories::alert_sources::{
            PgAlertSourceRepository, PgIntakeRequestRepository, PgMailboxRepository,
            PgSourceIncidentRepository,
        },
    },
};

pub(super) fn build(
    settings: &Settings,
    core: &Core,
    alerting: &AlertingRuntime,
    iam: &IamRuntime,
    query: &QueryRuntime,
) -> Arc<AlertSources> {
    let forwarder: Arc<dyn AlertForwarder> = Arc::new(GrpcAlertForwarder::new(
        query.remote_clusters.clone(),
        query.cluster_org_link.clone(),
        query.cluster_secrets.clone(),
        (settings.grpc.max_message_size as usize).saturating_mul(1024 * 1024),
    ));

    let mailboxes: Arc<dyn crate::domain::alerting::source::MailboxRepository> = Arc::new(
        PgMailboxRepository::new(core.pool.clone(), core.cipher_root_key.clone()),
    );
    let mail_client: Arc<dyn MailboxClient> = Arc::new(ImapMailboxClient);

    let sources = AlertSources::new(AlertSourcesParts {
        sources: Arc::new(PgAlertSourceRepository::new(core.pool.clone())),
        requests: Arc::new(PgIntakeRequestRepository::new(core.pool.clone())),
        source_incidents: Arc::new(PgSourceIncidentRepository::new(core.pool.clone())),
        incidents: core.incidents.clone(),
        escalations: core.escalations.clone(),
        evaluator: alerting.evaluator.clone(),
        alerting: alerting.alerting.clone(),
        cluster: Arc::new(InstanceSettingsCluster(iam.instance_settings.clone())),
        remote_clusters: query.remote_clusters.clone(),
        mailboxes: mailboxes.clone(),
    })
    .with_forwarder(forwarder)
    .with_mail_client(mail_client.clone());

    let sources = Arc::new(sources);

    // 邮件来源的邮箱：在告警节点上读。
    if core.roles.run_alert {
        MailboxPoller::new(
            sources.clone(),
            mailboxes,
            mail_client,
            core.alert_ownership.clone(),
        )
        .spawn();
    }

    sources
}
