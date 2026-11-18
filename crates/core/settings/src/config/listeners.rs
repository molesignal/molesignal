// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 跨段的监听端口校验：启用的节点级 pprof listener（`[profiling]`）不能与其它 listener
//! 共用端口，否则节点会在启动到一半时因端口已被占用而退出。

use super::Settings;

impl Settings {
    /// 这份配置会起的其它 listener 端口及其配置键，不含 `[profiling]`。
    ///
    /// 不按 `node.roles` 筛选：同一份配置常由各角色共用（Kubernetes ConfigMap、Compose），
    /// 端口冲突应在任一节点加载配置时报出，而不是只让起了那个 listener 的角色启动失败。
    /// 整段关闭的 listener（`flight_sql.enabled`、`http.tls.enabled` 为 false）不占端口。
    fn listener_ports(&self) -> Vec<(&'static str, u16)> {
        let mut ports = vec![
            ("http.port", self.http.port),
            ("grpc.port", self.grpc.port),
            ("otlp_grpc.port", self.otlp_grpc.port),
            ("probe.register_port", self.probe.register_port),
            ("probe.control_port", self.probe.control_port),
        ];

        if self.flight_sql.enabled {
            ports.push(("flight_sql.port", self.flight_sql.port));
        }

        if self.http.tls.enabled {
            ports.push(("http.tls.plain_port", self.http.tls.plain_port));
            ports.push(("http.tls.port", self.http.tls.port));
        }

        ports
    }

    /// 启用的 pprof listener 必须独占端口。在 `ProfilingSettings::validate` 之后调用，
    /// 那里已保证启用时端口大于 0。
    pub(super) fn validate_profiling_port(&self) -> anyhow::Result<()> {
        if !self.profiling.enabled {
            return Ok(());
        }

        let port = self.profiling.port;

        let clash = self
            .listener_ports()
            .into_iter()
            .find(|&(_, other)| other == port);

        if let Some((setting, _)) = clash {
            anyhow::bail!(
                "profiling.port and {setting} are both {port}; choose a different profiling.port"
            );
        }

        Ok(())
    }
}
