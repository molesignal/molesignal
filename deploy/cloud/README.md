# 云平台接入模板

把云平台的日志、指标和事件接入 MoleSignal 的部署模板。每个模板只在云平台一侧建资源，MoleSignal 一侧的连接器在 **流水线 → 连接器** 中创建，字段说明见文档站的「连接器」页。

| 目录 | 工具 | 建什么 | 对应的连接器 |
|---|---|---|---|
| [`aws/`](aws/cloudformation.yaml) | CloudFormation | Firehose 流（HTTP 端点目标）、CloudWatch 指标流（OpenTelemetry 1.0）、CloudWatch Logs 订阅（单个日志组或账号级订阅策略）、存放投递失败记录的 S3 存储桶 | Amazon Data Firehose |
| [`gcp/`](gcp/main.tf) | Terraform | 日志路由器接收器、Pub/Sub 主题和订阅（推送或拉取）；可选的读指标服务账号 | Google Cloud Pub/Sub 推送或拉取，Google Cloud Monitoring 指标 |
| [`azure/`](azure/main.bicep) | Bicep | Standard 层 Event Hubs 命名空间、事件中心、消费组、导出与读取两条授权规则 | Kafka 主题（`messageFormat: azure_records`） |

## 前提

- 推送方式要求 MoleSignal 能从云平台经公网 HTTPS 访问，证书由公共 CA 签发；Firehose 只投递到 443 端口。
- 先在 MoleSignal 中创建对应的推送连接器，复制它只显示一次的 Token，再把 Token 作为模板参数传入。

## AWS

```bash
aws cloudformation deploy --template-file aws/cloudformation.yaml --stack-name molesignal --capabilities CAPABILITY_IAM --parameter-overrides MoleSignalUrl=https://molesignal.example.com PushToken=<token> SubscribeAllLogGroups=true
```

- `StreamMetrics` 默认开启；`MetricNamespace` 留空时推送全部命名空间，需要多个命名空间时在控制台编辑指标流的过滤条件。
- `LogGroupName` 订阅单个日志组，`SubscribeAllLogGroups=true` 用账号级订阅策略订阅全部日志组。
- 指标流按每千次指标更新计费，Firehose 按数据量计费，见 AWS 定价页。

## Google Cloud

```bash
terraform -chdir=gcp init
```

```bash
terraform -chdir=gcp apply -var project_id=my-project -var molesignal_url=https://molesignal.example.com -var push_token=<token>
```

- 日志量大时用 `-var mode=pull`，再为输出的服务账号创建密钥，交给 Google Cloud Pub/Sub 拉取连接器。推送订阅每条消息一个请求。
- `-var metrics_reader=true` 另建一个有 `roles/monitoring.viewer` 的服务账号，供 Google Cloud Monitoring 指标连接器使用。
- 模板不生成服务账号密钥，以免私钥进入 Terraform state：

```bash
gcloud iam service-accounts keys create key.json --iam-account=<输出的服务账号>
```

## Azure

```bash
az deployment group create --resource-group <资源组> --template-file azure/main.bicep
```

输出的 `brokers`、`topic`、`connectionString` 填进 Kafka 主题连接器：打开 TLS，认证选 SASL/PLAIN，用户名填 `$ConnectionString`，密码填 `connectionString`，消息格式选 **Azure Monitor 导出**。

把资源的日志和指标导出到这个事件中心：

```bash
az monitor diagnostic-settings create --name molesignal --resource <资源 ID> --event-hub molesignal --event-hub-rule <输出的 exportRuleId> --logs '[{"categoryGroup":"allLogs","enabled":true}]' --metrics '[{"category":"AllMetrics","enabled":true}]'
```

把订阅的活动日志导出到这个事件中心：

```bash
az monitor diagnostic-settings subscription create --name molesignal --location <区域> --event-hub-name molesignal --event-hub-auth-rule <输出的 exportRuleId> --logs '[{"category":"Administrative","enabled":true},{"category":"Security","enabled":true},{"category":"ServiceHealth","enabled":true},{"category":"Alert","enabled":true},{"category":"Policy","enabled":true}]'
```

诊断设置导出的指标不带维度。需要按维度拆分的指标，用 Azure Monitor 指标连接器拉取，它需要一个在订阅上有 Monitoring Reader 的应用注册：

```bash
az ad sp create-for-rbac --name molesignal-metrics --role "Monitoring Reader" --scopes /subscriptions/<订阅 ID>
```

命令输出的 `tenant`、`appId`、`password` 分别填进连接器的租户 ID、客户端 ID、客户端密钥。

## 校验

这些模板只按各工具的文档编写，没有在真实账号上部署过。部署前可以先校验语法：

```bash
aws cloudformation validate-template --template-body file://aws/cloudformation.yaml
```

```bash
terraform -chdir=gcp validate
```

```bash
az bicep build --file azure/main.bicep
```
