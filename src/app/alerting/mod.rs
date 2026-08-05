// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警用例：规则 CRUD、Incident 处理、排班查询、升级派发编排。

pub mod dispatcher;
pub mod rule_evaluator;
pub mod service;

pub use dispatcher::EscalationDispatcher;
pub use rule_evaluator::RuleEvaluator;
pub use service::AlertingService;
