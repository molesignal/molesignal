# Kubernetes 部署

这套清单把各角色拆成独立的工作负载。`router` 提供完整的 HTTP API 与 Web UI，把写入转发给
`intake`、把查询交给 `querier`；其余角色在 5080 端口提供探活、就绪与指标。

## 资源清单

| 资源 | Kind | 副本 | 监听端口 | 就绪探针 | 说明 |
|---|---|---|---|---|---|
| `router`        | Deployment  | 2 | HTTP 5080、OTLP gRPC 4317 | `GET /api/v1/healthz` | 无状态、HPA 横向扩；完整 HTTP API 与 Web UI，写入经内部 gRPC 转发给 `intake`，查询交给 `querier`，并按组织限流 |
| `intake`        | StatefulSet | 2 | HTTP 5080、gRPC 5082、OTLP gRPC 4317 | `GET /api/v1/readyz` | 需本地盘存 WAL（每副本 20Gi）；监听在 WAL replay 完成后才开始 |
| `querier`       | Deployment  | 2 | HTTP 5080、gRPC 5082 | `GET /api/v1/readyz` | 无状态、HPA；经 gRPC 接收其它节点转来的查询，并执行搜索 job |
| `compactor`     | Deployment  | 1 | HTTP 5080 | `GET /api/v1/readyz` | 纯后台角色；可加副本，多个 compactor 按数据流与组织分担工作，节点故障后由其余节点接管 |
| `alert` | Deployment  | 1 | HTTP 5080、Probe 注册 5084、Probe 控制 5085 | `GET /api/v1/readyz` | 可加副本：多个 alert 节点按规则、报告、pipeline 分担，单点任务只在主节点运行，节点故障后由其余节点接管；同时承担 scheduled reports 的渲染 |
| `web`           | Deployment  | 2 | HTTP 8080 | `GET /healthz` | 可选的 nginx 静态资源层，附带 `web` Ingress |

5080 端口在 `router` 上是完整的 HTTP 服务，在其它角色上是内部运维监听：
`GET /api/v1/healthz`（子系统降级时返回 `503`）、`GET /api/v1/readyz`（WAL replay 完成前返回 `503`）
与 `GET /metrics`，不鉴权，也没有业务 API，不要暴露到集群外。

所有角色共享同一镜像 `molesignal:dev`，差别只在 `MS_NODE_ROLES`
环境变量（覆盖 ConfigMap 里的 `[node].roles`）。每个角色启动时都会对对象存储做一次读写自检，
所以 `router` 与其它角色一样需要对象存储凭据。

交付通道也不参与镜像构建。`10-configmap.yaml` 的 `release_channel` 会注入为
`RELEASE_CHANNEL`；从 `alpha` 晋升到 `beta`、`rc` 或 `stable` 时继续使用同一个
`BUILD_ID` 对应的不可变镜像，只更新该值并滚动部署。

## 依赖

清单不包含 PostgreSQL / RustFS；自行部署或换托管：
- PostgreSQL 17，默认 Service/DNS 名为 `postgresql`；DSN 写到
  `molesignal-config` ConfigMap 的 `meta_database.dsn`
- 任意 S3 兼容对象存储；endpoint / bucket / region 写到 `object_store`，
  Access Key 与 Secret Key 通过 `molesignal-secret` 注入。

## 部署顺序

```bash
kubectl apply -f 00-namespace.yaml
kubectl apply -f 10-configmap.yaml
# 生产环境替换对象存储凭据、cipher root key 与集群 token；JWT secret 默认由 DB 自动 bootstrap。
kubectl create secret generic molesignal-secret -n molesignal \
  --from-literal=object_store_access_key="<access-key>" \
  --from-literal=object_store_secret_key="<secret-key>" \
  --from-literal=cipher_key="$(openssl rand -base64 32)" \
  --from-literal=self_telemetry_cluster_token="$(openssl rand -base64 32)" \
  --dry-run=client -o yaml | kubectl apply -f -
# 或 dev 临时用 20-secret.yaml
kubectl apply -f 30-router.yaml -f 40-intake.yaml -f 50-querier.yaml \
              -f 60-compactor.yaml -f 70-alert.yaml
# 可选：nginx 静态资源层与 Ingress
kubectl apply -f 80-web.yaml
```

拆分角色的节点之间转发 Trace 候选与自遥测时，用共享的 `MS_SELF_TELEMETRY_CLUSTER_TOKEN` 鉴权。
Trace 采集默认开启，所以每个工作负载都从 `molesignal-secret` 的 `self_telemetry_cluster_token`
读取同一个值：没有 intake 角色的节点缺少它会拒绝启动，intake 与 querier 缺少它会拒收其它节点转来的
Trace 候选。

## 验证

```bash
kubectl -n molesignal get pods -l app=molesignal
```

各角色 Pod 的 `READY` 都为 `1/1` 后，`router` 的健康检查是公开的：

```bash
kubectl -n molesignal port-forward svc/router 5080:5080
curl http://127.0.0.1:5080/api/v1/healthz
```
