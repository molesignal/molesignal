import { renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';

import i18n from '@/i18n';
import enText from '@/i18n/en-us/datasources.json';

import { SOURCES } from './sources';
import { useSourceText } from './sourceText';

const LOCALES = ['en-us', 'zh-cn'] as const;

describe('the catalogue and its words', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
  });

  it.each(LOCALES)('names and describes every source and titles every step in %s', (locale) => {
    const has = (key: string) => i18n.exists(key, { ns: 'datasources', lng: locale });

    for (const source of SOURCES) {
      expect(has(`${source.id}.name`), `${source.id}.name`).toBe(true);
      expect(has(`${source.id}.description`), `${source.id}.description`).toBe(true);
      source.steps.forEach((_step, index) => {
        const key = `${source.id}.steps.${index + 1}.title`;
        expect(has(key), key).toBe(true);
      });
    }
  });

  it('has words for nothing the catalogue does not hold', () => {
    const text = enText as Record<string, { steps: Record<string, unknown> }>;
    const steps = new Map(SOURCES.map((source) => [source.id, source.steps.length]));

    expect(Object.keys(text).sort()).toEqual([...steps.keys()].sort());
    for (const [id, entry] of Object.entries(text)) {
      expect(Object.keys(entry.steps), id).toHaveLength(steps.get(id) ?? 0);
    }
  });

  it('gives every step something to read or something to copy', () => {
    for (const source of SOURCES) {
      source.steps.forEach((step, index) => {
        const described = i18n.exists(`${source.id}.steps.${index + 1}.description`, {
          ns: 'datasources',
        });
        expect(Boolean(step.code) || described, `${source.id} step ${index + 1}`).toBe(true);
      });
    }
  });

  it('reads a source in the language in use', async () => {
    const { result, rerender } = renderHook(() => useSourceText());
    const linux = { id: 'linux' };

    expect(result.current.name(linux)).toBe('Linux hosts');
    expect(result.current.step(linux, 0)).toEqual({ title: 'Install Vector' });

    await i18n.changeLanguage('zh-cn');
    rerender();

    expect(result.current.name(linux)).toBe('Linux 主机');
    expect(result.current.step(linux, 0)).toEqual({ title: '安装 Vector' });
  });

  it('puts this workspace\'s address into a sentence that names the endpoint', () => {
    const { result } = renderHook(() => useSourceText());

    const forward = result.current.step({ id: 'gcp' }, 1, {
      ENDPOINT: 'https://obs.example.com',
    });

    expect(forward.description).toContain(
      'POST to https://obs.example.com/api/v1/intake/logs/default',
    );
  });

  it.each(LOCALES)('never asks prose to carry the token in %s', (locale) => {
    const bundle = JSON.stringify(i18n.getResourceBundle(locale, 'datasources'));

    expect(bundle).not.toContain('{{TOKEN}}');
  });

  it('keeps an optional note only where the step has one', () => {
    const { result } = renderHook(() => useSourceText());

    expect(result.current.step({ id: 'rum' }, 2).note).toMatch(/^Do not use the RUM client token/);
    expect(result.current.step({ id: 'rum' }, 0)).not.toHaveProperty('note');
    expect(result.current.step({ id: 'rum' }, 0)).not.toHaveProperty('description');
  });
});
