import { http } from '@/lib/http';

export interface InstanceInfo {
  /** Configured external URL; empty means the UI should fall back to window.location.origin. */
  external_url: string;
  /** 是否开放自助注册：signin 页据此显示「注册」入口（公开读，无需认证）。 */
  signup_enabled: boolean;
  /** Backend product version, injected by the Rust build. */
  version: string;
  /** Runtime deployment maturity; it does not affect the Rust binary. */
  release_channel: string;
}

export async function get(): Promise<InstanceInfo> {
  const { data } = await http.get<InstanceInfo>('/instance');
  return data;
}

export interface SignupPolicy {
  signup_enabled: boolean;
  signup_require_approval: boolean;
}

/** 读取自助注册策略（`org.settings.read`）。 */
export async function getSignupPolicy(): Promise<SignupPolicy> {
  const { data } = await http.get<SignupPolicy>('/settings/signup');
  return data;
}

/** 更新自助注册策略（`org.settings.manage`）。 */
export async function updateSignupPolicy(policy: SignupPolicy): Promise<SignupPolicy> {
  const { data } = await http.put<SignupPolicy>('/settings/signup', policy);
  return data;
}

export type ServiceGraphSource = 'ingest' | 'storage';

export interface ServiceGraphSettings {
  /** ingest（各进程内存配对+flush）或 storage（单例 worker 从存储重算，跨节点正确）。 */
  source: ServiceGraphSource;
}

/** 读取服务图数据来源模式（`org.settings.read`）。 */
export async function getServiceGraphSettings(): Promise<ServiceGraphSettings> {
  const { data } = await http.get<ServiceGraphSettings>('/settings/service_graph');
  return data;
}

/** 更新服务图数据来源模式（`org.settings.manage`）。 */
export async function updateServiceGraphSettings(
  settings: ServiceGraphSettings,
): Promise<ServiceGraphSettings> {
  const { data } = await http.put<ServiceGraphSettings>('/settings/service_graph', settings);
  return data;
}

export interface FederationSettings {
  /** 本集群稳定唯一 id（事件 source/writer）；非空 = 启用联邦，留空 = 关闭。 */
  cluster_id: string;
  /** outbox drain → 推送远端周期（秒）。 */
  drain_interval_secs: number;
  /** 单次推送批量上限。 */
  push_batch_size: number;
  /** 接收端去重表保留窗口（秒）。 */
  seen_events_ttl_secs: number;
  /** 集群拓扑 gossip 周期（秒）。 */
  gossip_interval_secs: number;
}

/** 读取跨集群联邦配置（`org.settings.read`）。 */
export async function getFederationSettings(): Promise<FederationSettings> {
  const { data } = await http.get<FederationSettings>('/settings/federation');
  return data;
}

/** 更新跨集群联邦配置（`org.settings.manage`）。 */
export async function updateFederationSettings(
  settings: FederationSettings,
): Promise<FederationSettings> {
  const { data } = await http.put<FederationSettings>('/settings/federation', settings);
  return data;
}

export type ClientIpMode = 'peer' | 'header' | 'forwarded_chain';

export interface ClientIpResolverSettings {
  mode: ClientIpMode;
  header_name: string;
  trusted_proxy_cidrs: string[];
  fallback_to_peer: boolean;
  allow_private_client_ips: boolean;
  max_chain_length: number;
}

/** 读取部署级 RUM 客户端 IP 识别配置。 */
export async function getClientIpSettings(): Promise<ClientIpResolverSettings> {
  const { data } = await http.get<ClientIpResolverSettings>('/settings/client_ip');
  return data;
}

/** 更新部署级 RUM 客户端 IP 识别配置（`sys.settings.manage`）。 */
export async function updateClientIpSettings(
  settings: ClientIpResolverSettings,
): Promise<ClientIpResolverSettings> {
  const { data } = await http.put<ClientIpResolverSettings>('/settings/client_ip', settings);
  return data;
}
