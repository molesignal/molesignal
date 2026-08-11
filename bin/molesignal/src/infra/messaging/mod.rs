// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 节点间消息总线。standalone 模式下用 in-process channel；
//! 集群模式下可接 NATS / Redis Streams 等。

pub trait MessageBus: Send + Sync {
    fn publish(&self, topic: &str, payload: Vec<u8>);
}
