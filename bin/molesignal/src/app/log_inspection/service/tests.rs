// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use super::*;
use crate::{
    app::log_inspection::{
        input::tests::{input, limits},
        test_support::{Agent, Channels, Model, Store, Streams, member},
    },
    domain::{iam::IamScope, log_inspection::InspectionSchedule},
};

const MANAGE: [&str; 3] = ["agent.use", "streams.query", "inspections.manage"];

struct Fixture {
    store: Arc<Store>,
    service: LogInspectionService,
}

fn fixture() -> Fixture {
    fixture_with(Channels::default(), limits())
}

fn fixture_with(channels: Channels, limits: InspectionLimits) -> Fixture {
    let store = Store::with([]);

    let service = LogInspectionService::new(
        store.clone(),
        store.clone(),
        Arc::new(Streams(&["app_logs"])),
        Arc::new(channels),
        Agent::new(Model::Reports("{}", &[])),
        limits,
    );

    Fixture { store, service }
}

fn alice() -> IamContext {
    member("alice", &MANAGE)
}

#[tokio::test]
async fn a_saved_inspection_runs_as_its_saver_and_is_due_one_period_later() {
    let fixture = fixture();
    let before = TimestampMicros::now();

    let saved = fixture.service.create(&alice(), input()).await.unwrap();

    assert_eq!(saved.name, "payments");
    assert_eq!(saved.run_as_user_id.0, "alice");
    assert_eq!(saved.created_by.0, "alice");
    assert_eq!(saved.timezone, "Asia/Shanghai");
    assert!(saved.last_run.is_none());

    // 每小时一次：下一次在一小时后。
    let next = saved
        .next_run_at
        .expect("an enabled inspection is scheduled");
    assert!(next.0 >= before.0 + 3_600_000_000);
    assert!(next.0 <= TimestampMicros::now().0 + 3_600_000_000);
    assert_eq!(fixture.store.inspection(&saved.id.0).name, "payments");
}

#[tokio::test]
async fn an_inspection_saved_switched_off_is_not_scheduled() {
    let fixture = fixture();

    let saved = fixture
        .service
        .create(
            &alice(),
            InspectionInput {
                enabled: false,
                ..input()
            },
        )
        .await
        .unwrap();

    assert_eq!(saved.next_run_at, None);
}

#[tokio::test]
async fn saving_takes_a_signed_in_member_who_could_run_it_themselves() {
    let fixture = fixture();

    let token = IamContext {
        scope: IamScope::ApiToken,
        ..alice()
    };

    assert!(matches!(
        fixture.service.create(&token, input()).await,
        Err(Error::Forbidden(_))
    ));

    // 能管理巡检，但自己查不了数据流：存进去也是一执行就失败。
    let cannot_query = member("bob", &["agent.use", "inspections.manage"]);

    let refused = fixture
        .service
        .create(&cannot_query, input())
        .await
        .unwrap_err();

    assert!(refused.to_string().contains("streams.query"), "{refused}");
    assert_eq!(fixture.store.inspection_count(), 0);
}

#[tokio::test]
async fn what_the_inspection_refers_to_must_exist() {
    let fixture = fixture_with(
        Channels {
            missing_connector: Some("connector-1"),
            ..Channels::default()
        },
        limits(),
    );

    let mut silent = input();
    silent.notify_routes.clear();

    // 通知渠道指向一个不存在的连接器。
    let refused = fixture.service.create(&alice(), input()).await.unwrap_err();
    assert!(matches!(refused, Error::InvalidArgument(_)), "{refused}");
    assert!(refused.to_string().contains("connector-1"));

    // 数据流不是这个组织的日志数据流。
    let refused = fixture
        .service
        .create(
            &alice(),
            InspectionInput {
                stream: "no_such_stream".into(),
                ..silent.clone()
            },
        )
        .await
        .unwrap_err();
    assert!(refused.to_string().contains("no_such_stream"), "{refused}");

    // 模型配置用不了。
    let refused = fixture
        .service
        .create(
            &alice(),
            InspectionInput {
                provider_id: "missing".into(),
                ..silent.clone()
            },
        )
        .await
        .unwrap_err();
    assert!(refused.to_string().contains("AI model"), "{refused}");

    assert!(fixture.service.create(&alice(), silent).await.is_ok());
}

#[tokio::test]
async fn an_organization_keeps_at_most_as_many_inspections_as_the_instance_allows() {
    let fixture = fixture_with(
        Channels::default(),
        InspectionLimits {
            max_per_org: 2,
            ..limits()
        },
    );

    for name in ["one", "two"] {
        let named = InspectionInput {
            name: name.into(),
            ..input()
        };

        fixture.service.create(&alice(), named).await.unwrap();
    }

    let third = InspectionInput {
        name: "three".into(),
        ..input()
    };

    assert!(matches!(
        fixture.service.create(&alice(), third).await,
        Err(Error::Conflict(_))
    ));
}

#[tokio::test]
async fn a_name_is_used_once_in_an_organization() {
    let fixture = fixture();
    fixture.service.create(&alice(), input()).await.unwrap();

    let again = fixture.service.create(&alice(), input()).await.unwrap_err();

    assert_eq!(
        again.to_string(),
        "conflict: a scheduled log inspection with this name already exists"
    );
}

#[tokio::test]
async fn saving_again_hands_the_inspection_to_the_new_saver_and_keeps_its_schedule() {
    let fixture = fixture();
    let saved = fixture.service.create(&alice(), input()).await.unwrap();

    let renamed = InspectionInput {
        name: "payments (prod)".into(),
        ..input()
    };

    let updated = fixture
        .service
        .update(&member("carol", &MANAGE), &saved.id, renamed)
        .await
        .unwrap();

    // 执行身份改成这次保存的人，创建者不变。
    assert_eq!(updated.run_as_user_id.0, "carol");
    assert_eq!(updated.created_by.0, "alice");
    assert_eq!(updated.name, "payments (prod)");

    // 周期没变，下次执行时间不重新起算。
    assert_eq!(updated.next_run_at, saved.next_run_at);
}

#[tokio::test]
async fn a_changed_schedule_is_counted_from_the_save() {
    let fixture = fixture();
    let saved = fixture.service.create(&alice(), input()).await.unwrap();

    let two_hourly = InspectionInput {
        schedule: InspectionSchedule::Interval { every_minutes: 120 },
        ..input()
    };

    let updated = fixture
        .service
        .update(&alice(), &saved.id, two_hourly)
        .await
        .unwrap();

    let next = updated.next_run_at.unwrap();
    assert!(next.0 >= saved.next_run_at.unwrap().0 + 3_600_000_000);
}

#[tokio::test]
async fn switching_off_and_on_keeps_the_identity_and_reschedules() {
    let fixture = fixture();
    let saved = fixture.service.create(&alice(), input()).await.unwrap();
    let org = saved.org_id.clone();

    let off = fixture
        .service
        .set_enabled(&org, &saved.id, false)
        .await
        .unwrap();
    assert!(!off.enabled);
    assert_eq!(off.next_run_at, None);

    let on = fixture
        .service
        .set_enabled(&org, &saved.id, true)
        .await
        .unwrap();
    assert!(on.enabled);
    assert!(on.next_run_at.is_some());
    assert_eq!(on.run_as_user_id.0, "alice");
}

#[tokio::test]
async fn a_manual_run_is_queued_for_the_requester_and_only_one_at_a_time() {
    let fixture = fixture();
    let saved = fixture.service.create(&alice(), input()).await.unwrap();
    let dave = member("dave", &MANAGE);

    let queued = fixture.service.run_now(&dave, &saved.id).await.unwrap();

    assert_eq!(queued.status, RunStatus::Queued);
    assert_eq!(queued.trigger, RunTrigger::Manual);
    assert_eq!(queued.requested_by.as_ref().unwrap().0, "dave");
    assert!(queued.started_at.is_none());

    // 执行用的仍然是保存者的身份，不是点按钮的人。
    assert_eq!(queued.run_as_user_id.as_ref().unwrap().0, "alice");

    let again = fixture.service.run_now(&dave, &saved.id).await.unwrap_err();

    assert_eq!(
        again.to_string(),
        "conflict: this log inspection already has a run in progress"
    );
}

#[tokio::test]
async fn another_organization_cannot_see_run_or_delete_an_inspection() {
    let fixture = fixture();
    let saved = fixture.service.create(&alice(), input()).await.unwrap();

    let outsider = IamContext {
        org_id: Id::from_string("org-2"),
        ..member("mallory", &MANAGE)
    };

    let other = &outsider.org_id;

    assert!(matches!(
        fixture.service.get(other, &saved.id).await,
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        fixture.service.run_now(&outsider, &saved.id).await,
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        fixture.service.list_runs(other, &saved.id, None, 50).await,
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        fixture.service.delete(other, &saved.id).await,
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        fixture.service.update(&outsider, &saved.id, input()).await,
        Err(Error::NotFound(_))
    ));
    assert!(fixture.service.list(other).await.unwrap().is_empty());
}

#[tokio::test]
async fn deleting_an_inspection_takes_its_runs_with_it() {
    let fixture = fixture();
    let saved = fixture.service.create(&alice(), input()).await.unwrap();
    fixture.service.run_now(&alice(), &saved.id).await.unwrap();

    fixture
        .service
        .delete(&saved.org_id, &saved.id)
        .await
        .unwrap();

    assert!(fixture.store.runs().is_empty());
    assert_eq!(fixture.store.inspection_count(), 0);
}
