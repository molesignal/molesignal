import { createHash } from 'node:crypto';

export const API_BASE = (
  process.env.MS_SEED_API ?? 'http://127.0.0.1:5080/api/v1'
).replace(/\/$/, '');
export const EMAIL = process.env.MS_SEED_EMAIL ?? 'admin@example.com';
export const PASSWORD = process.env.MS_SEED_PASSWORD ?? 'admin';
export const NOW_MS = Date.now();
export const RUN_ID = new Date(NOW_MS).toISOString().replace(/\D/g, '').slice(0, 14);

export class Api {
  constructor(base) {
    this.base = base;
    this.token = '';
    this.orgId = '';
    this.userId = '';
  }

  async request(method, path, body, headers = {}) {
    const response = await fetch(`${this.base}${path}`, {
      method,
      headers: {
        accept: 'application/json',
        ...(body === undefined ? {} : { 'content-type': 'application/json' }),
        ...(this.token ? { authorization: `Bearer ${this.token}` } : {}),
        ...headers,
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await response.text();
    const data = text ? JSON.parse(text) : null;
    if (!response.ok) {
      throw new Error(
        `${method} ${path} -> ${response.status} ${data?.message ?? text}`,
      );
    }
    return data;
  }

  async login() {
    const session = await this.request('POST', '/auth/signin', {
      email: EMAIL,
      password: PASSWORD,
    });
    this.token = session.token;
    this.orgId = session.org_id;
    this.userId = session.user_id;
    const orgs = await this.request('GET', '/orgs');
    const selected =
      orgs.find((org) => org.slug === 'default' || org.name === 'default') ??
      orgs[0];
    if (selected && selected.id !== this.orgId) {
      const next = await this.request('POST', `/orgs/${selected.id}/select`, {});
      this.token = next.token;
      this.orgId = next.org_id;
      this.userId = next.user_id ?? this.userId;
    }
  }

  post(path, body) {
    return this.request('POST', path, body);
  }
}

export const SERVICES = {
  'api-gateway': {
    language: 'go',
    sdkVersion: '1.35.0',
    stable: '2.8.2',
    candidate: '2.8.3',
    instances: 4,
  },
  'order-service': {
    language: 'rust',
    sdkVersion: '0.28.0',
    stable: '4.12.0',
    candidate: '4.13.0',
    instances: 6,
  },
  'inventory-service': {
    language: 'go',
    sdkVersion: '1.35.0',
    stable: '1.18.4',
    candidate: '1.19.0',
    instances: 3,
  },
  'payment-service': {
    language: 'java',
    sdkVersion: '2.15.0',
    stable: '3.6.1',
    candidate: '3.7.0',
    instances: 5,
  },
  'user-service': {
    language: 'nodejs',
    sdkVersion: '2.0.0',
    stable: '5.4.0',
    candidate: '5.4.1',
    instances: 3,
  },
  'notification-service': {
    language: 'python',
    sdkVersion: '1.35.0',
    stable: '2.2.0',
    candidate: '2.3.0',
    instances: 4,
  },
};

export const SERVICE_NAMES = Object.keys(SERVICES);
export const kv = (key, value) => ({
  key,
  value:
    typeof value === 'number'
      ? Number.isInteger(value)
        ? { intValue: value }
        : { doubleValue: value }
      : typeof value === 'boolean'
        ? { boolValue: value }
        : { stringValue: String(value) },
});
const digest = (seed, size) =>
  createHash('sha256').update(seed).digest('hex').slice(0, size);
export const traceId = (index) =>
  digest(`molesignal-website-${RUN_ID}-trace-${index}`, 32);
export const spanId = (trace, name) => digest(`${trace}-${name}`, 16);
export const ns = (millis) =>
  String(BigInt(Math.floor(millis)) * 1_000_000n);

export function versionFor(service, index) {
  const config = SERVICES[service];
  return index % 5 === 0 ? config.candidate : config.stable;
}
