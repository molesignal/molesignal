# Kubernetes 部署

## 资源清单

| 资源 | Kind | 副本 | 说明 |
|---|---|---|---|
| `router`        | Deployment  | 2 | 无状态、HPA 横向扩 |
| `ingester`      | StatefulSet | 2 | 需本地盘存 WAL（每副本 20Gi） |
| `querier`       | Deployment  | 2 | 无状态、HPA |
| `compactor`     | Deployment  | 1 | 单实例避免合并冲突；后续上 lease 表锁再加副本 |
| `alert-manager` | Deployment  | 1 | 同上 |

所有角色共享同一镜像 `molesignal:dev`，差别只在 `MS_NODE_ROLES`
环境变量（覆盖 ConfigMap 里的 `[node].roles`）。

交付通道也不参与镜像构建。`10-configmap.yaml` 的 `release_channel` 会注入为
`RELEASE_CHANNEL`；从 `alpha` 晋升到 `beta`、`rc` 或 `stable` 时继续使用同一个
`BUILD_ID` 对应的不可变镜像，只更新该值并滚动部署。

## 依赖

清单不包含 PostgreSQL / MinIO；自行部署或换托管：
- PostgreSQL 17，默认 Service/DNS 名为 `postgresql`；DSN 写到
  `molesignal-config` ConfigMap 的 `store.meta.dsn`
- 任意 S3 兼容对象存储；endpoint / bucket / region 写到 `store.object`，
  Access Key 与 Secret Key 通过 `molesignal-secret` 注入。

## 部署顺序

```bash
kubectl apply -f 00-namespace.yaml
kubectl apply -f 10-configmap.yaml
# 生产环境替换对象存储凭据和 cipher root key；JWT secret 默认由 DB 自动 bootstrap。
kubectl create secret generic molesignal-secret -n molesignal \
  --from-literal=object_store_access_key="<access-key>" \
  --from-literal=object_store_secret_key="<secret-key>" \
  --from-literal=cipher_key="$(openssl rand -base64 32)" \
  --dry-run=client -o yaml | kubectl apply -f -
# 或 dev 临时用 20-secret.yaml
kubectl apply -f 30-router.yaml -f 40-ingester.yaml -f 50-querier.yaml \
              -f 60-compactor.yaml -f 70-alert-manager.yaml
```

## 验证

```bash
kubectl -n molesignal port-forward svc/router 5080:5080
curl http://127.0.0.1:5080/api/v1/healthz
```
