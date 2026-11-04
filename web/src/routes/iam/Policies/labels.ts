import type { TFunction } from 'i18next';

/** What names a policy: a built-in one in the interface language, a custom one as written. */
export interface NamedPolicy {
  key: string;
  name: string;
  description: string;
  builtin: boolean;
}

export function policyName(policy: NamedPolicy, t: TFunction<'iam'>): string {
  if (!policy.builtin) return policy.name;
  return t(`policies.builtin.${policy.key}.name`, { defaultValue: policy.name });
}

export function policyDescription(policy: NamedPolicy, t: TFunction<'iam'>): string {
  if (!policy.builtin) return policy.description;
  return t(`policies.builtin.${policy.key}.description`, { defaultValue: policy.description });
}
