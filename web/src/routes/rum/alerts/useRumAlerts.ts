import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import * as alertsApi from '@/api/alerts';
import type { AlertRuleInput } from '@/api/alerts';
import * as incidentsApi from '@/api/incidents';
import * as rumApi from '@/api/rum';
import { toApiError } from '@/lib/http';
import { toast } from '@/shell/ui/sonner';
import { useAuthStore } from '@/stores/auth';

import { type AlertTemplate, type TemplateScope, templateQuery } from './templates';

/** How often the incidents going on are read again while a page is open. */
const INCIDENTS_REFRESH_MS = 60_000;

/** The incidents going on, which the button of every page counts and the drawer lists. */
export function useActiveIncidents(enabled: boolean) {
  return useQuery({
    queryKey: ['rum', 'alerts', 'incidents'],
    queryFn: () => incidentsApi.list({ scope: 'active' }),
    enabled,
    refetchInterval: INCIDENTS_REFRESH_MS,
    retry: false,
  });
}

/** The alert rules of the organization: the rule editor and the other modules read them under the same key. */
export function useAlertRules(enabled: boolean) {
  return useQuery({
    queryKey: ['alerts', 'rules'],
    queryFn: () => alertsApi.list(),
    enabled,
    retry: false,
  });
}

/**
 * What a template's figure is now: the preview of the rule the template would make, over the one
 * window that ends now. It is read by the code that evaluates the rule, so a value over a line is
 * what the rule would find at once.
 */
export function useCurrentFigure(template: AlertTemplate, scope: TemplateScope, enabled: boolean) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: ['rum', 'alerts', 'current', orgId, scope.application, scope.environment, template.id],
    queryFn: ({ signal }) => {
      const to = Date.now() * 1_000;

      return rumApi.previewAlert(
        { query: templateQuery(template, scope), periodSecs: template.window, from: to - template.window * 1_000_000, to },
        signal,
      );
    },
    enabled: Boolean(orgId) && enabled,
    staleTime: 60_000,
    retry: false,
  });
}

/** Creates a rule; the rules are read again before the call is over, so the card is already in its new state. */
export function useCreateRule() {
  const { t } = useTranslation('rum');
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (rule: AlertRuleInput) => alertsApi.create(rule),
    onSuccess: async (saved) => {
      toast.success(t('alerts.templates.created_toast', { name: saved.name }));
      await queryClient.invalidateQueries({ queryKey: ['alerts', 'rules'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
}
