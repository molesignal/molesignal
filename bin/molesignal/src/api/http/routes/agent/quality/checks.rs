// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Prompt 晋级检查。
//!
//! - `POST /api/v1/agent/settings/prompts/{id}/checks`：开始检查一个组织 Prompt。
//! - `GET  /api/v1/agent/settings/prompts/{id}/gate`：设为默认之前的判定与最近的检查。
//!
//! 设为默认时由 [`gate_set_default`] 把门；检查由 worker 用 [`process_next`] 做，每做完一个样本
//! 记一次进度。规则见 `agent::scoring::check`。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::Serialize;
use serde_json::json;

use super::{
    evaluating::{self, Model},
    facts::{gather, readable},
    replay,
};
use crate::{
    agent::scoring::{
        Evaluator, GateMode, TargetSource,
        check::{
            CheckSample, CheckStatus, Gate, MAX_CHECK_SAMPLES, PromptCheck, checked_sources, gate,
        },
        evaluation::Material,
    },
    api::{AppState, http::routes::activity_audit, rca::system_user_id},
    app::iam::IamContext,
    domain::iam::permission,
    infra::persistence::repositories::agent::prompts::AgentPromptTemplate,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 领走的检查多久没有进度算节点没了。
const LEASE_MICROS: i64 = 10 * 60 * 1_000_000;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/settings/prompts/{id}/checks", post(start))
        .route("/agent/settings/prompts/{id}/gate", get(gate_view))
}

fn lease() -> TimestampMicros {
    TimestampMicros(TimestampMicros::now().0 + LEASE_MICROS)
}

/// 评这些来源的第一个启用的评审。
fn evaluator_for<'a>(
    evaluators: &'a [Evaluator],
    sources: &[TargetSource],
) -> Option<&'a Evaluator> {
    evaluators
        .iter()
        .find(|evaluator| sources.iter().any(|source| evaluator.scores(*source)))
}

/// 本组织的一个组织 Prompt，和它要比的现在的默认。
async fn prompt_and_default(
    state: &AppState,
    ctx: &IamContext,
    id: &str,
) -> Result<(AgentPromptTemplate, AgentPromptTemplate)> {
    let prompt = state.agent.prompts.get(&Id(id.to_string())).await?;

    if prompt.org_id.as_deref() != Some(ctx.org_id.0.as_str()) {
        return Err(Error::not_found(format!("prompt `{id}` not found")));
    }

    let default = state
        .agent
        .prompts
        .resolve(&ctx.org_id, &system_user_id(), &prompt.purpose)
        .await?;

    Ok((prompt, default))
}

/// 设为默认之前的判定。
struct Decision {
    gate: Gate,
    mode: GateMode,
    check: Option<PromptCheck>,
    current: bool,
}

async fn decide(
    state: &AppState,
    org_id: &Id,
    prompt: &AgentPromptTemplate,
    default: &AgentPromptTemplate,
    confirmed: bool,
) -> Result<Decision> {
    let mode = state.agent.scoring.settings(org_id).await?.gate;
    let check = state.agent.scoring.latest_check(org_id, &prompt.id).await?;
    let current = check
        .as_ref()
        .is_some_and(|check| check.current(prompt.version, (&default.id, default.version)));

    let evaluated = match checked_sources(&prompt.purpose) {
        Some(sources) => {
            let evaluators = state.agent.scoring.list_evaluators(org_id).await?;
            evaluator_for(&evaluators, sources).is_some()
        }
        None => false,
    };

    // 已经是默认的不用再比。
    let gate = if prompt.scope != "org" || prompt.id == default.id {
        Gate::Allow
    } else {
        gate(
            mode,
            &prompt.purpose,
            check.as_ref().map(|check| (check, current)),
            evaluated,
            confirmed,
        )
    };

    Ok(Decision {
        gate,
        mode,
        check,
        current,
    })
}

/// 设为默认之前把门：要再确认或被拒绝时回 409，说明里是原因的代码。
pub(crate) async fn gate_set_default(
    state: &AppState,
    ctx: &IamContext,
    prompt: &AgentPromptTemplate,
    confirmed: bool,
) -> Result<()> {
    let default = state
        .agent
        .prompts
        .resolve(&ctx.org_id, &system_user_id(), &prompt.purpose)
        .await?;

    match decide(state, &ctx.org_id, prompt, &default, confirmed)
        .await?
        .gate
    {
        Gate::Allow => Ok(()),
        Gate::Confirm(reason) => Err(Error::conflict(format!(
            "{}: confirm to set this prompt as the default anyway",
            reason.as_str()
        ))),
        Gate::Refuse(reason) => Err(Error::conflict(reason.as_str())),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GateResp {
    mode: GateMode,
    /// 这个用途在检查范围里。
    checked: bool,
    /// `allow` | `confirm` | `refuse`
    decision: &'static str,
    /// `prompt_check_required` | `prompt_check_running` | `prompt_check_worse` |
    /// `prompt_check_unavailable`
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    /// 最近一次检查；`current` 表示它检查的还是现在的版本与默认。
    check: Option<PromptCheck>,
    current: bool,
    default_prompt_id: String,
}

#[permission("agent.prompts.manage")]
async fn gate_view(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<GateResp>> {
    let (prompt, default) = prompt_and_default(&state, &ctx, &id).await?;
    let decision = decide(&state, &ctx.org_id, &prompt, &default, false).await?;

    let (verdict, reason) = match decision.gate {
        Gate::Allow => ("allow", None),
        Gate::Confirm(reason) => ("confirm", Some(reason.as_str())),
        Gate::Refuse(reason) => ("refuse", Some(reason.as_str())),
    };

    Ok(Json(GateResp {
        mode: decision.mode,
        checked: checked_sources(&prompt.purpose).is_some(),
        decision: verdict,
        reason,
        check: decision.check,
        current: decision.current,
        default_prompt_id: default.id.0,
    }))
}

#[permission("agent.prompts.manage")]
async fn start(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<PromptCheck>> {
    let (prompt, default) = prompt_and_default(&state, &ctx, &id).await?;

    if prompt.scope != "org" {
        return Err(Error::invalid("only an organization prompt is checked"));
    }

    let sources = checked_sources(&prompt.purpose)
        .ok_or_else(|| Error::invalid("prompts for this purpose are not checked"))?;

    if prompt.id == default.id {
        return Err(Error::invalid("this prompt is already the default"));
    }

    // 同一个版本与默认正在检查时不再开一次。
    if let Some(check) = state
        .agent
        .scoring
        .latest_check(&ctx.org_id, &prompt.id)
        .await?
        .filter(|check| {
            matches!(check.status, CheckStatus::Queued | CheckStatus::Running)
                && check.current(prompt.version, (&default.id, default.version))
        })
    {
        return Ok(Json(check));
    }

    let evaluators = state.agent.scoring.list_evaluators(&ctx.org_id).await?;
    let evaluator = evaluator_for(&evaluators, sources)
        .ok_or_else(|| Error::invalid("add an evaluator that scores these answers first"))?;

    let check = PromptCheck {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        prompt_id: prompt.id.clone(),
        prompt_version: prompt.version,
        purpose: prompt.purpose.clone(),
        baseline_prompt_id: default.id.clone(),
        baseline_version: default.version,
        evaluator_id: evaluator.id.clone(),
        status: CheckStatus::Queued,
        samples: Vec::new(),
        baseline_mean: None,
        candidate_mean: None,
        verdict: None,
        error: None,
        tokens: 0,
        requested_by: ctx.user_id.clone(),
        created_at: TimestampMicros::now(),
        finished_at: None,
    };

    state.agent.scoring.create_check(&check).await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.prompt.check_started",
        "agent_prompt",
        &prompt.id.0,
        json!({
            "purpose": prompt.purpose,
            "version": prompt.version,
            "defaultPromptId": default.id.0,
            "evaluatorId": evaluator.id.0,
        }),
    )
    .await;

    Ok(Json(check))
}

/// 领走一次排着的检查并做完；没有排着的返回 `false`。
pub(crate) async fn process_next(state: &AppState) -> Result<bool> {
    let scoring = &state.agent.scoring;

    let Some(mut check) = scoring.claim_check(TimestampMicros::now(), lease()).await? else {
        return Ok(false);
    };

    if let Err(error) = run(state, &mut check).await {
        check.status = CheckStatus::Failed;
        check.error = Some(error.to_string());
        check.finished_at = Some(TimestampMicros::now());
    }

    scoring.save_check(&check, lease()).await?;
    Ok(true)
}

async fn run(state: &AppState, check: &mut PromptCheck) -> Result<()> {
    let scoring = &state.agent.scoring;
    let org_id = check.org_id.clone();
    let candidate = state.agent.prompts.get(&check.prompt_id).await?;

    if candidate.version != check.prompt_version {
        return Err(Error::conflict("the prompt was edited during the check"));
    }

    let baseline = state.agent.prompts.get(&check.baseline_prompt_id).await?;
    let evaluator = scoring.get_evaluator(&org_id, &check.evaluator_id).await?;
    let sources = checked_sources(&check.purpose)
        .ok_or_else(|| Error::invalid("prompts for this purpose are not checked"))?;

    if check.samples.is_empty() {
        check.samples = samples(state, &org_id, &evaluator, sources).await?;
        scoring.save_check(check, lease()).await?;
    }

    let model = evaluating::model(state, &org_id, None, None).await?;

    for index in 0..check.samples.len() {
        let sample = &check.samples[index];

        if sample.scored().is_some() || sample.error.is_some() {
            continue;
        }

        if state.agent.usage.gate(&org_id, false).await.refused() {
            return Err(Error::unavailable(
                "today's Mole Agent token budget is used up",
            ));
        }

        if let Err(error) = score_sample(
            state,
            check,
            index,
            (&evaluator, &model),
            (&baseline, &candidate),
        )
        .await
        {
            check.samples[index].error = Some(error.to_string());
        }

        scoring.save_check(check, lease()).await?;
    }

    check.conclude();
    check.status = CheckStatus::Done;
    check.finished_at = Some(TimestampMicros::now());
    Ok(())
}

/// 样本：先取回归集，再取评审最近评过的，同一个对象只取一次。
async fn samples(
    state: &AppState,
    org_id: &Id,
    evaluator: &Evaluator,
    sources: &[TargetSource],
) -> Result<Vec<CheckSample>> {
    let scoring = &state.agent.scoring;
    let wanted = MAX_CHECK_SAMPLES as i64;
    let mut picked: Vec<CheckSample> = Vec::new();

    let sample = |kind, target_id: String, regression| CheckSample {
        target_kind: kind,
        target_id,
        regression,
        baseline: None,
        candidate: None,
        baseline_reason: String::new(),
        candidate_reason: String::new(),
        error: None,
    };

    for review in scoring.regression_set(org_id, sources, wanted).await? {
        picked.push(sample(review.target_kind, review.target_id, true));
    }

    for target in scoring
        .recent_evaluated(org_id, &evaluator.id, sources, wanted * 2)
        .await?
    {
        if picked.len() >= MAX_CHECK_SAMPLES {
            break;
        }

        let subject = target.subject;
        let seen = picked.iter().any(|picked| {
            picked.target_kind == subject.target_kind && picked.target_id == subject.target_id
        });

        if !seen {
            picked.push(sample(subject.target_kind, subject.target_id, false));
        }
    }

    Ok(picked)
}

/// 一个样本：两个版本各回答一次，同一个评审打分。
async fn score_sample(
    state: &AppState,
    check: &mut PromptCheck,
    index: usize,
    (evaluator, model): (&Evaluator, &Model),
    (baseline, candidate): (&AgentPromptTemplate, &AgentPromptTemplate),
) -> Result<()> {
    let scoring = &state.agent.scoring;
    let org_id = check.org_id.clone();
    let (kind, target_id, regression) = {
        let sample = &check.samples[index];
        (
            sample.target_kind,
            sample.target_id.clone(),
            sample.regression,
        )
    };

    let scored = scoring
        .scored(&org_id, kind, &target_id)
        .await?
        .ok_or_else(|| Error::not_found("the answer is no longer scored"))?;

    let facts = gather(
        state,
        &org_id,
        kind,
        &target_id,
        scored.subject.attribution.chat_id.as_ref(),
    )
    .await?
    .ok_or_else(|| Error::not_found("the answer is gone"))?;

    // 回归集里的对象带上审核人的备注：好的回答应该答对什么。
    let note = if regression {
        scoring
            .review_of(&org_id, kind, &target_id)
            .await?
            .map(|review| review.note)
            .filter(|note| !note.trim().is_empty())
    } else {
        None
    };

    let mut verdicts = Vec::new();

    for template in [baseline, candidate] {
        let (answer, tokens) = replay::answer_with(state, &org_id, template, &facts, model).await?;
        let answer = readable(&answer);
        let material = Material {
            answer: &answer,
            ..facts.material(note.as_deref())
        };
        let evaluated = evaluating::evaluate(state, evaluator, &material).await?;

        check.tokens += tokens + evaluated.tokens;
        verdicts.push(evaluated.verdict);
    }

    let sample = &mut check.samples[index];
    let candidate = verdicts.pop().expect("two verdicts");
    let baseline = verdicts.pop().expect("two verdicts");

    sample.baseline = Some(baseline.score);
    sample.baseline_reason = baseline.reason;
    sample.candidate = Some(candidate.score);
    sample.candidate_reason = candidate.reason;
    Ok(())
}
