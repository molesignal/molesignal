// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Google Cloud: pulling Cloud Monitoring metrics ([`monitoring`]), reading Pub/Sub subscriptions
//! by push or pull ([`pubsub`]), and the service account key both sign in with
//! ([`service_account`]).

pub mod monitoring;
pub mod pubsub;
pub mod service_account;
