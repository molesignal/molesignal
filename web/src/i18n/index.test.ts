import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import i18n, { SUPPORTED_LOCALES } from './index';

describe('i18n', () => {
  it('resolves bundled resources for lowercase regional locale ids', async () => {
    await i18n.changeLanguage('en-us');
    expect(i18n.t('common:actions.sign_in')).toBe('Sign in');
    expect(i18n.t('shell:signin.version', { version: '26.0.0.0' })).toBe('v26.0.0.0');
    expect(i18n.t('shell:about.editions.oss')).toBe('OpenSource Edition');
    expect(i18n.t('shell:about.editions.enterprise')).toBe('Enterprise Edition');
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
    expect(i18n.t('iam:groups.toast_deleted')).toBe('访问授权已删除');
    expect(i18n.t('rum:upload_source_maps.uploading')).toBe('上传中…');
  });
});

/* ─── completeness: every locale against en-us ─────────────────────────────── */

type Bundle = Record<string, unknown>;

// Read from disk rather than from i18next: only English is loaded up front.
const FILES = import.meta.glob<Bundle>('./*/*.json', { eager: true, import: 'default' });

function bundlesOf(locale: string): Record<string, Bundle> {
  const bundles: Record<string, Bundle> = {};
  for (const [path, content] of Object.entries(FILES)) {
    const match = /^\.\/([^/]+)\/([^/]+)\.json$/.exec(path);
    if (match?.[1] === locale && match[2]) bundles[match[2]] = content;
  }
  return bundles;
}

const EN = bundlesOf('en-us');
const TARGETS = SUPPORTED_LOCALES.filter((locale) => locale !== 'en-us');

// CLDR plural categories. i18next emits `key_one` / `key_other` (etc.) per the
// locale's plural rules — English has `_one` + `_other`, Chinese only `_other`,
// French also `_many`. Strip the suffix so a value pluralized differently in
// one locale doesn't register as a missing key, while genuinely absent keys
// still surface.
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

describe('builtin tool catalog translations', () => {
  const enCatalog = leafValue(EN.agent ?? {}, 'tools') as Bundle;

  it.each(TARGETS)('contains complete %s metadata for all builtin tools', (locale) => {
    const catalog = leafValue(bundlesOf(locale).agent ?? {}, 'tools') as Bundle;
    const builtinToolKinds = readFileSync(
      resolve('../crates/modules/tool-runtime/src/catalog/kind.rs'),
      'utf8',
    );
    const builtinNames = Array.from(
      builtinToolKinds.matchAll(/\b\w+\s*=>\s*"([^"]+)"/g),
      ([, name]) => name,
    ).sort();
    expect(builtinNames.length).toBeGreaterThan(0);
    expect(Object.keys(enCatalog).sort()).toEqual(builtinNames);
    expect(Object.keys(catalog ?? {}).sort()).toEqual(builtinNames);

    for (const [name, metadata] of Object.entries(enCatalog)) {
      expect(name).toMatch(/^[a-z][a-z0-9_]*$/);
      expect(metadata).toMatchObject({
        title: expect.any(String),
        description: expect.any(String),
      });
      expect(catalog[name]).toMatchObject({
        title: expect.any(String),
        description: expect.any(String),
      });
    }
  });
});

describe('i18n resource consistency', () => {
  const namespaces = Object.keys(EN).sort();
  const cases = TARGETS.flatMap((locale) => namespaces.map((ns) => [locale, ns] as const));

  it.each(TARGETS)('bundles every namespace for %s', (locale) => {
    expect(namespaces.length).toBeGreaterThan(0);
    expect(Object.keys(bundlesOf(locale)).sort()).toEqual(namespaces);
  });

  it.each(cases)('%s namespace "%s" has the keys of en-us', (locale, ns) => {
    const en = normalizedKeySet(EN[ns] ?? {});
    const target = normalizedKeySet(bundlesOf(locale)[ns] ?? {});
    const missing = [...en].filter((k) => !target.has(k)).sort();
    const extra = [...target].filter((k) => !en.has(k)).sort();
    expect({ locale, ns, missing, extra }).toEqual({ locale, ns, missing: [], extra: [] });
  });

  it.each(SUPPORTED_LOCALES)('every %s leaf value is a non-empty string', (locale) => {
    const data = bundlesOf(locale);
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
