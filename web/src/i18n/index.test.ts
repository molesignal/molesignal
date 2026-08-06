import { describe, expect, it } from 'vitest';

import i18n from './index';

describe('i18n', () => {
  it('resolves bundled resources for lowercase regional locale ids', async () => {
    await i18n.changeLanguage('en-us');
    expect(i18n.t('common:actions.sign_in')).toBe('Sign in');
    expect(i18n.t('shell:signin.version_line')).not.toBe('signin.version_line');
    expect(i18n.t('shell:about.editions.oss')).toBe('OpenSource Edition');
    expect(i18n.t('shell:about.editions.enterprise')).toBe('Enterprise Edition');
    expect(i18n.t('design-system:states.license-gated.title')).toBe('Pro feature');
    expect(i18n.t('edition:gates.pro-required.title')).toBe('Pro required');
    expect(i18n.t('onboarding:activation.title')).toBe('Quick start');
    expect(i18n.t('product:templates.overview')).toBe('Overview');
    expect(i18n.t('alerts:title')).toBe('Alerts');
    expect(i18n.t('dashboards:title')).toBe('Dashboards');
    expect(i18n.t('pipelines:title')).toBe('Pipelines');
    expect(i18n.t('reports:title')).toBe('Reports');
    expect(i18n.t('streams:title')).toBe('Streams');
    expect(i18n.t('traces:detail.title')).toBe('Trace detail');
    expect(i18n.t('logs:inspector.title')).toBe('Log inspector');
    expect(i18n.t('settings-admin:subtitle')).toBe(
      'Manage organization, security, data plane, and automation capabilities.',
    );
    expect(i18n.t('settings-admin:license.labels.max_ingest_bytes_per_day')).toBe(
      'Max ingest per day (bytes)',
    );
    expect(i18n.t('iam:users.toast_removed')).toBe(
      'Member removed from the current workspace',
    );
    expect(i18n.t('rum:source_maps.toast_deleted')).toBe('Deleted');

    await i18n.changeLanguage('zh-cn');
    expect(i18n.t('common:actions.sign_in')).toBe('登录');
    expect(i18n.t('shell:about.editions.oss')).toBe('OpenSource Edition');
    expect(i18n.t('shell:about.editions.enterprise')).toBe('Enterprise Edition');
    expect(i18n.t('settings-admin:nav.general')).toBe('通用');
    expect(i18n.t('design-system:states.permission-denied.title')).toBe('需要权限');
    expect(i18n.t('edition:gates.saas-only.title')).toBe('需要 SaaS 账号');
    expect(i18n.t('onboarding:datasource.endpoint')).toBe('端点');
    expect(i18n.t('product:templates.list')).toBe('列表');
    expect(i18n.t('alerts:title')).toBe('告警');
    expect(i18n.t('dashboards:title')).toBe('仪表盘');
    expect(i18n.t('pipelines:actions.new_pipeline')).toBe('新建流水线');
    expect(i18n.t('reports:title')).toBe('报告中心');
    expect(i18n.t('streams:title')).toBe('数据流');
    expect(i18n.t('traces:detail.title')).toBe('链路详情');
    expect(i18n.t('logs:inspector.title')).toBe('日志检查器');
    expect(i18n.t('settings-admin:subtitle')).toBe(
      '管理组织、安全、数据面和自动化能力。',
    );
    expect(i18n.t('settings-admin:license.labels.edition')).toBe('版本');
    expect(i18n.t('settings-admin:license.labels.verified')).toBe('已验证');
    expect(i18n.t('settings-admin:license.labels.expired')).toBe('已过期');
    expect(i18n.t('settings-admin:license.labels.issued_to')).toBe('授权对象');
    expect(i18n.t('settings-admin:license.labels.features')).toBe('功能');
    expect(i18n.t('settings-admin:license.labels.max_ingest_bytes_per_day')).toBe(
      '每日最大写入量（字节）',
    );
    expect(i18n.t('settings-admin:license.labels.expires_at')).toBe('到期时间');
    expect(i18n.t('iam:groups.toast_deleted')).toBe('访问授权已删除');
    expect(i18n.t('rum:upload_source_maps.uploading')).toBe('上传中…');
  });
});

/* ─── completeness + en-us ↔ zh-cn parity (P2-T4) ─────────────────────────── */

type Bundle = Record<string, unknown>;

const EN = (i18n.getDataByLanguage('en-us') ?? {}) as Record<string, Bundle>;
const ZH = (i18n.getDataByLanguage('zh-cn') ?? {}) as Record<string, Bundle>;

// CLDR plural categories. i18next emits `key_one` / `key_other` (etc.) per the
// locale's plural rules — English has `_one` + `_other`, Chinese only `_other`.
// Strip the suffix so a value pluralized in only one locale doesn't register as
// a missing key, while genuinely absent keys still surface.
const PLURAL_SUFFIX = /_(zero|one|two|few|many|other)$/;

function leafKeys(obj: Bundle, prefix = ''): string[] {
  const out: string[] = [];
  for (const [k, v] of Object.entries(obj)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (v !== null && typeof v === 'object' && !Array.isArray(v)) {
      out.push(...leafKeys(v as Bundle, key));
    } else {
      out.push(key);
    }
  }
  return out;
}

function normalizedKeySet(bundle: Bundle): Set<string> {
  return new Set(leafKeys(bundle).map((k) => k.replace(PLURAL_SUFFIX, '')));
}

function leafValue(bundle: Bundle, dottedKey: string): unknown {
  return dottedKey.split('.').reduce<unknown>((o, part) => {
    if (o && typeof o === 'object') return (o as Bundle)[part];
    return undefined;
  }, bundle);
}

describe('i18n resource parity', () => {
  const namespaces = Object.keys(EN);

  it('bundles the same namespaces for both locales', () => {
    expect(namespaces.length).toBeGreaterThan(0);
    expect(Object.keys(ZH).sort()).toEqual([...namespaces].sort());
  });

  it.each(namespaces)('namespace "%s" has matching keys in en-us and zh-cn', (ns) => {
    const en = normalizedKeySet(EN[ns] ?? {});
    const zh = normalizedKeySet(ZH[ns] ?? {});
    const missingInZh = [...en].filter((k) => !zh.has(k)).sort();
    const missingInEn = [...zh].filter((k) => !en.has(k)).sort();
    expect({ ns, missingInZh, missingInEn }).toEqual({ ns, missingInZh: [], missingInEn: [] });
  });

  it.each(['en-us', 'zh-cn'] as const)('every %s leaf value is a non-empty string', (lng) => {
    const data = lng === 'en-us' ? EN : ZH;
    const offenders: string[] = [];
    for (const ns of Object.keys(data)) {
      const bundle = data[ns] ?? {};
      for (const key of leafKeys(bundle)) {
        const val = leafValue(bundle, key);
        if (typeof val !== 'string' || val.trim().length === 0) {
          offenders.push(`${ns}:${key} = ${JSON.stringify(val)}`);
        }
      }
    }
    expect(offenders).toEqual([]);
  });
});
