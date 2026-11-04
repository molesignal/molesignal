import { http } from '@/lib/http';

/** What a rule does with the errors it matches: one group per service named by the rule, or no group. */
export type ErrorRuleAction = 'merge' | 'ignore';

/**
 * A rule that groups errors otherwise than by their type, frame and endpoint.
 * Each pattern matches the whole text, ignoring case, with `*` for any text;
 * an empty one matches anything. An empty service name matches every service.
 */
export interface ErrorRule {
  id: string;
  name: string;
  serviceNamespace: string;
  serviceName: string;
  errorType: string;
  message: string;
  frame: string;
  endpoint: string;
  action: ErrorRuleAction;
  enabled: boolean;
  /** Where it is tried among the organization's rules, the lowest first. */
  position: number;
  updatedBy: string;
  updatedAt: number;
}

export type ErrorRuleWrite = Pick<ErrorRule, 'name' | 'serviceNamespace' | 'serviceName' | 'errorType' | 'message' | 'frame' | 'endpoint' | 'action' | 'enabled'>;

export const apmErrorRulesApi = {
  list: async () => {
    const { data } = await http.get<ErrorRule[]>('/apm/error-rules');
    return data;
  },
  create: async (write: ErrorRuleWrite) => {
    const { data } = await http.post<ErrorRule>('/apm/error-rules', write);
    return data;
  },
  update: async (id: string, write: ErrorRuleWrite) => {
    const { data } = await http.put<ErrorRule>(`/apm/error-rules/${encodeURIComponent(id)}`, write);
    return data;
  },
  remove: async (id: string) => {
    await http.delete(`/apm/error-rules/${encodeURIComponent(id)}`);
  },
  /** Tries the rules in the order of `ids`. */
  order: async (ids: string[]) => {
    const { data } = await http.put<ErrorRule[]>('/apm/error-rules/order', { ids });
    return data;
  },
};

export const apmErrorRulesQueryKeys = {
  list: (orgId: string) => ['apm', 'error-rules', orgId] as const,
};

/** Whether `pattern` matches the whole of `text`, ignoring case, `*` standing for any text: as the server matches. */
export function patternMatches(pattern: string, text: string): boolean {
  const p = [...pattern.toLowerCase()];
  const value = [...text.toLowerCase()];
  let at = 0;
  let from = 0;
  let star: [number, number] | null = null;

  while (at < value.length) {
    if (from < p.length && p[from] === '*') {
      star = [from, at];
      from += 1;
    } else if (from < p.length && p[from] === value[at]) {
      from += 1;
      at += 1;
    } else if (star) {
      from = star[0] + 1;
      at = star[1] + 1;
      star = [star[0], star[1] + 1];
    } else {
      return false;
    }
  }

  return p.slice(from).every((char) => char === '*');
}
