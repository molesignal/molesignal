import type { ProviderTemplate } from '@/api/notify/templates';

/** The providers SMS and phone templates carry a template for: they send no other. */
export const TEMPLATE_PROVIDERS = ['aliyun', 'tencent', 'huawei'] as const;

export type TemplateProvider = (typeof TEMPLATE_PROVIDERS)[number];

/** Tencent Cloud and Huawei Cloud fill a template's variables by position: they are named 1, 2, 3. */
export const FILLED_BY_POSITION: ReadonlySet<TemplateProvider> = new Set(['tencent', 'huawei']);

export type ProviderDrafts = Record<TemplateProvider, ProviderDraft>;

export function providerDrafts(templates: Record<string, ProviderTemplate> | undefined): ProviderDrafts {
  return {
    aliyun: providerDraft(templates?.aliyun),
    tencent: providerDraft(templates?.tencent),
    huawei: providerDraft(templates?.huawei),
  };
}

/** The first provider with something that keeps it from being saved. */
export function draftsProblem(drafts: ProviderDrafts): { provider: TemplateProvider; problem: ProviderProblem } | null {
  for (const provider of TEMPLATE_PROVIDERS) {
    const problem = providerProblem(drafts[provider]);
    if (problem) return { provider, problem };
  }

  return null;
}

export interface ProviderParam {
  name: string;
  value: string;
}

/** A provider template as the editor holds it. All empty is no provider template. */
export interface ProviderDraft {
  code: string;
  params: ProviderParam[];
  maxChars: string;
}

export function providerDraft(template: ProviderTemplate | undefined): ProviderDraft {
  return {
    code: template?.code ?? '',
    params: Object.entries(template?.params ?? {}).map(([name, value]) => ({ name, value })),
    maxChars: template?.maxParamChars ? String(template.maxParamChars) : '',
  };
}

export type ProviderProblem = 'code' | 'param_name' | 'param_twice' | 'param_value' | 'max_chars';

const WORD = /^[A-Za-z0-9_-]{1,64}$/;
const VARIABLE = /^[A-Za-z0-9_]{1,64}$/;

function written(draft: ProviderDraft): ProviderParam[] {
  return draft.params.filter((param) => param.name.trim() !== '' || param.value.trim() !== '');
}

/** What keeps a provider template from being saved, checked as the server checks it. */
export function providerProblem(draft: ProviderDraft): ProviderProblem | null {
  const params = written(draft);
  if (draft.code.trim() === '' && params.length === 0 && draft.maxChars.trim() === '') return null;
  if (!WORD.test(draft.code.trim())) return 'code';

  const names = params.map((param) => param.name.trim());
  if (names.some((name) => !VARIABLE.test(name))) return 'param_name';
  if (new Set(names).size !== names.length) return 'param_twice';
  if (params.some((param) => param.value.trim() === '')) return 'param_value';

  const max = draft.maxChars.trim();
  if (max !== '' && !(Number.isInteger(Number(max)) && Number(max) >= 1 && Number(max) <= 500)) {
    return 'max_chars';
  }

  return null;
}

/** The provider templates a template is saved with: none for an empty draft. */
export function providerTemplates(drafts: ProviderDrafts): Record<string, ProviderTemplate> {
  const saved: Record<string, ProviderTemplate> = {};

  for (const provider of TEMPLATE_PROVIDERS) {
    const draft = drafts[provider];
    const code = draft.code.trim();

    if (code === '') continue;

    const max = draft.maxChars.trim();

    saved[provider] = {
      code,
      params: Object.fromEntries(written(draft).map((param) => [param.name.trim(), param.value])),
      ...(max === '' ? {} : { maxParamChars: Number(max) }),
    };
  }

  return saved;
}
