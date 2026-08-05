# 节点级 pprof

MoleSignal 提供真实的 CPU/heap pprof 下载接口。profiling listener 在每种 node role 上独立启动，默认关闭并只绑定 loopback。

```toml
[profiling]
enabled = true
bind = "127.0.0.1"
port = 5084
allow_remote = false
```

兼容环境变量 `MS_PROFILING_ENABLED` 和 `MS_PROFILING_ALLOW_REMOTE`。`bind` 必须是 IP 地址；未启用 `allow_remote` 时，非 loopback 地址会导致配置校验失败。

## 接口与命令

```bash
# 采集 30 秒 CPU profile；seconds 有效范围为 1..120
curl --fail --output cpu.pb.gz \
  'http://127.0.0.1:5084/debug/pprof/profile?seconds=30'

go tool pprof cpu.pb.gz

# 当前 heap profile
curl --fail --output heap.pb.gz \
  'http://127.0.0.1:5084/debug/pprof/heap'

go tool pprof heap.pb.gz
```

响应是 gzip 压缩的 canonical `perftools.profiles.Profile` protobuf，content type 为 `application/octet-stream`。同一进程一次只允许一个 CPU capture；并发请求返回 `409 Conflict` 和 `Retry-After: 1`。无效 duration 返回 `400`，当前构建不支持某类 profile 时返回 `501`。

主 API 保留兼容别名：

- `GET /api/v1/debug/profile/cpu?seconds=N`
- `GET /api/v1/debug/profile/heap`

别名与独立 listener 共用 capture service 和响应格式，但因为主 API 可能对外暴露，始终要求 Owner 或 Admin bearer。

## 远程暴露与敏感性

pprof 可能暴露函数名、代码布局、资源使用模式和内存分配信息。推荐保持 loopback，通过 SSH tunnel 或节点本地运维 agent 抓取。

当确需远程监听时，同时设置非 loopback `bind` 和 `allow_remote=true`。每个请求仍必须携带管理组织的 Owner/Admin JWT 或 API token：

```bash
curl --fail \
  -H "Authorization: Bearer ${MOLESIGNAL_ADMIN_TOKEN}" \
  --output cpu.pb.gz \
  'https://node.example:5084/debug/pprof/profile?seconds=10'
```

还应使用防火墙、NetworkPolicy 或安全组限制来源，并在 listener 前提供 TLS；`allow_remote` 本身不是传输加密开关。

## 平台支持

| profile | 支持范围 | 不支持时 |
|---|---|---|
| CPU | `profiling-pprof` feature + Unix；发布矩阵覆盖 x86_64 Linux、aarch64 Linux、aarch64 macOS | `501` |
| heap | Linux glibc + `jemalloc` + `profiling-pprof` | `501` |

发布构建保留最小调试符号并强制 frame pointer，保证受支持目标可 unwind。heap adapter 把 jemalloc `heap_v2` dump 转为 canonical pprof；其他 allocator/平台不会返回伪造的成功 profile。

## 开销

- CPU sampler 默认 99 Hz，只在请求或 scheduled capture 的时间窗内采样。较长 duration 会增加 profile 数据量和少量 unwind 开销。
- 第一次 heap capture 会启用 jemalloc profiling，采样随后保持到进程退出；这会带来持续的 allocator 采样和额外内存开销。
- 符号与 frame pointer 会增加发布二进制体积。
- profile 内容可能很大，抓取频率应低于 metrics/logs。

开启 `telemetry.self_collect.enabled` 后，scheduled self profiles 使用 `profile_kinds`、`profile_interval_secs` 和 `profile_duration_secs`。它与 HTTP capture 共用进程级互斥；竞争时 scheduled capture 被跳过并计入 drop metric。成功的 on-demand capture 在响应完整生成后异步归档到 `profiles/_molesignal`，归档失败不会截断下载。
