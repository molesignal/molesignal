// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 报表渲染的应用层：一份 PDF/PNG 报表渲染时，浏览器以谁的身份取数。
//!
//! 即时导出和分享快照以发起请求的用户渲染（[`render_session`]）；定时投递没有发起请求的人，
//! 以最后保存报表的登录用户渲染（[`ScheduledReportIdentity`]）。这位用户是谁，随报表一起
//! 返回给界面（[`run_as_profiles`]）。

mod render_session;
mod run_as_profile;
mod scheduled_identity;

pub use render_session::{RENDER_SESSION_TTL_SECS, ReportRenderSession, render_session};
pub use run_as_profile::{RunAsProfile, run_as_profiles};
pub use scheduled_identity::{ScheduledReportIdentity, run_as_user};
