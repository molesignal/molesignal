import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Sparkles } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import * as incidentsApi from '@/api/incidents';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { MarkdownMessage } from '@/routes/agent/markdown';
import { ChromeButton } from '@/shell/chrome';
import { LoadingState } from '@/shell/LoadingState';
import { toast } from '@/shell/ui/sonner';

import { IncidentSection } from './Section';

/**
 * AI root-cause analysis. The backend generates one for an active incident
 * within a couple of minutes; the button asks for one now
 * (`POST /alerts/incidents/{id}/rca`). Until one exists the read returns
 * nothing, which is not an error.
 */
export function IncidentRcaSection({ incidentId, surface = false }: { incidentId: string; surface?: boolean }) {
  const { t, i18n } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const locale = incidentsApi.normalizeRcaLocale(i18n.resolvedLanguage ?? i18n.language);

  const query = useQuery({
    queryKey: ['incident-rca', incidentId],
    queryFn: () => incidentsApi.getRca(incidentId),
    enabled: !!incidentId,
    retry: false,
    staleTime: 30_000,
  });
  const generate = useMutation({
    mutationFn: () => incidentsApi.generateRca(incidentId, locale),
    onSuccess: (rca) => {
      queryClient.setQueryData(['incident-rca', incidentId], rca);
      toast.success(t('drawer.rca.generated'));
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });

  const rca = query.data ?? null;
  const generateButton = (
    <ChromeButton
      size="sm"
      onClick={() => generate.mutate()}
      disabled={generate.isPending}
      data-testid="incident-rca-generate"
    >
      <Sparkles className="h-3 w-3" />
      {generate.isPending
        ? t('drawer.rca.generating')
        : rca
          ? t('drawer.rca.regenerate')
          : t('drawer.rca.generate')}
    </ChromeButton>
  );

  return (
    <IncidentSection
      title={t('drawer.incident_sections.rca')}
      icon={<Sparkles className="h-3.5 w-3.5" />}
      surface={surface}
    >
      {query.isLoading ? (
        <LoadingState variant="list" rows={2} />
      ) : rca ? (
        <div className="flex flex-col gap-2">
          <div className="rounded-md bg-bg-2 px-3 py-2.5">
            <MarkdownMessage
              content={rca.summary}
              className="font-sans text-xs [&_h1]:text-sm [&_h2]:text-sm [&_h3]:text-xs [&_h4]:text-xs"
            />
          </div>
          <div className="flex items-center gap-2 font-sans text-xs text-tx-2">
            {rca.model && <span className="truncate">{rca.model}</span>}
            <span className="ml-auto tabular-nums">{formatMicrosActive(rca.created_at)}</span>
            {generateButton}
          </div>
        </div>
      ) : (
        <div className="flex flex-col items-start gap-2">
          <p className="font-sans text-xs text-tx-2">{t('drawer.rca.empty')}</p>
          {generateButton}
        </div>
      )}
    </IncidentSection>
  );
}
