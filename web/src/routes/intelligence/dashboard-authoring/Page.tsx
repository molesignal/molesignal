import { LayoutDashboard } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import {
  useDashboardDraft,
  useExecuteDashboardCreation,
  useProposeDashboardCreation,
} from '@/api/intelligence/dashboardAuthoring';
import { DashboardRenderer } from '@/dashboard-engine/DashboardRenderer';
import { parseDashboardDefinition } from '@/dashboard-engine/model';
import { toApiError } from '@/lib/http';
import { ProductState } from '@/product/states';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { toast } from '@/shell/ui/sonner';
import { useAuthStore } from '@/stores/auth';

import { DraftStatusPanel } from './DraftStatusPanel';
import { dashboardCreationIdempotencyKey } from './model';

export function DashboardDraftPage() {
  const { t } = useTranslation('intelligence');
  const { id = '' } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const draftQuery = useDashboardDraft(id);
  const propose = useProposeDashboardCreation();
  const execute = useExecuteDashboardCreation(id);
  const [nowMicros, setNowMicros] = React.useState(() => Date.now() * 1000);
  const actionInFlight = React.useRef(false);

  React.useEffect(() => {
    const timer = window.setInterval(() => setNowMicros(Date.now() * 1000), 1000);
    return () => window.clearInterval(timer);
  }, []);

  const parsedModel = React.useMemo(() => {
    if (!draftQuery.data) return { dashboard: null, error: null };
    try {
      return {
        dashboard: parseDashboardDefinition(draftQuery.data.compiled_model),
        error: null,
      };
    } catch (error) {
      return { dashboard: null, error };
    }
  }, [draftQuery.data]);

  const submitProposal = () => {
    const draft = draftQuery.data;
    if (!draft || propose.isPending || actionInFlight.current) return;
    actionInFlight.current = true;
    propose.mutate(
      {
        draftId: draft.draft_id,
        expectedHash: draft.model_hash,
        reason: t('dashboard_authoring.proposal_reason'),
        impact: t('dashboard_authoring.proposal_impact'),
      },
      {
        onSuccess: () => toast.success(t('dashboard_authoring.proposal_created')),
        onError: (error) => toast.error(toApiError(error).message),
        onSettled: () => {
          actionInFlight.current = false;
        },
      },
    );
  };

  const confirmCreate = () => {
    const draft = draftQuery.data;
    const approvalId = draft?.operation?.approval_id;
    if (!draft || !approvalId || execute.isPending || actionInFlight.current) return;
    actionInFlight.current = true;
    execute.mutate(
      {
        approvalId,
        idempotencyKey: dashboardCreationIdempotencyKey(
          draft.draft_id,
          draft.model_hash,
        ),
      },
      {
        onSuccess: (result) => {
          if (result.status !== 'succeeded') {
            toast.error(result.error || t('dashboard_authoring.creation_failed'));
            return;
          }
          const route = result.verification.dashboard_route;
          toast.success(t('dashboard_authoring.created'));
          navigate(route || '/dashboards');
        },
        onError: (error) => toast.error(toApiError(error).message),
        onSettled: () => {
          actionInFlight.current = false;
        },
      },
    );
  };

  const title = draftQuery.data?.compiled_model.title || t('dashboard_authoring.title');
  return (
    <>
      <PageHeader
        title={title}
        subtitle={t('dashboard_authoring.subtitle')}
        backTo="/intelligence/chat"
        breadcrumbs={null}
      />
      <PageBody className="p-3 sm:p-4 xl:p-5">
        {draftQuery.isLoading ? (
          <ProductState
            variant="loading"
            title={t('dashboard_authoring.loading_title')}
            description={t('dashboard_authoring.loading_description')}
          />
        ) : draftQuery.isError ? (
          <ProductState
            variant="error"
            title={t('dashboard_authoring.load_failed')}
            description={toApiError(draftQuery.error).message}
          />
        ) : parsedModel.error || !parsedModel.dashboard || !draftQuery.data ? (
          <ProductState
            variant="error"
            title={t('dashboard_authoring.invalid_preview')}
            description={t('dashboard_authoring.invalid_preview_description')}
          />
        ) : (
          <div className="grid items-start gap-4 xl:grid-cols-[minmax(0,1fr)_340px]">
            <section
              aria-label={t('dashboard_authoring.preview_title')}
              className="min-w-0 overflow-hidden rounded-md border border-bd-0 bg-bg-1"
            >
              <div className="flex items-center gap-2 border-b border-bd-0 bg-bg-2 px-4 py-3">
                <LayoutDashboard className="h-4 w-4 text-indigo" />
                <div>
                  <h2 className="text-sm font-display-strong text-tx-0">
                    {t('dashboard_authoring.preview_title')}
                  </h2>
                  <p className="mt-0.5 text-xs text-tx-3">
                    {t('dashboard_authoring.preview_description')}
                  </p>
                </div>
              </div>
              <div className="min-h-[420px] overflow-auto p-3 sm:p-4">
                <DashboardRenderer
                  dashboard={parsedModel.dashboard}
                  orgId={orgId}
                  restricted
                />
              </div>
            </section>
            <DraftStatusPanel
              draft={draftQuery.data}
              nowMicros={nowMicros}
              busy={propose.isPending || execute.isPending}
              onPropose={submitProposal}
              onExecute={confirmCreate}
            />
          </div>
        )}
      </PageBody>
    </>
  );
}
