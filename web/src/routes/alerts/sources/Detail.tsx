import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowUpRight, KeyRound, Pencil, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate, useParams } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import * as alertSourcesApi from '@/api/alertSources';
import type { IssuedAlertSource } from '@/api/alertSources';
import * as incidentsApi from '@/api/incidents';
import * as instanceApi from '@/api/instance';
import { toApiError } from '@/lib/http';
import { resolvePublicBaseUrl } from '@/lib/publicUrl';
import { useActionAccess } from '@/product/actionAccess';
import { productStateFor, type ProductStateProps } from '@/product/states';
import { DetailPage } from '@/product/templates';
import { CopyableValue, ReadOnlyValue } from '@/routes/settings/_atoms';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { ALERT_SOURCES_KEY, SourceDrawer } from './Drawer';
import { SOURCE_KIND_ICONS } from './kindIcon';
import { MailboxSection } from './MailboxSection';
import { intakeAddress, setupSnippet, sourceState, tokenInAddress } from './model';
import { usePolicyName } from './policyName';
import { SourceRequests } from './Requests';
import { SECTION_INSET, SetupStep, SnippetBlock, SourceSection, ValueHint } from './Section';
import { SourceStanding } from './Standing';
import { SourceTokenDialog } from './TokenDialog';

/** Requests read for the day's figures; the list under them shows the latest of these. */
const REQUEST_PAGE = 200;
const LISTED_REQUESTS = 50;

/**
 * One alert source on a page of its own: how it stands and what the last day
 * brought, how its sender is set up, and what the sender has sent.
 *
 * The token itself is never on the page. A new source arrives here with it
 * once (in the navigation state of its creation), and rotating gives a new
 * one in the same way; otherwise the setup shows where the token goes.
 */
export function AlertsSourceDetail() {
  const { t, i18n } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const { id = '' } = useParams<{ id: string }>();
  const location = useLocation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.sources.manage' });
  const policyName = usePolicyName();
  const [editing, setEditing] = React.useState(false);
  const [rotating, setRotating] = React.useState(false);
  const [removing, setRemoving] = React.useState(false);
  const [issued, setIssued] = React.useState<IssuedAlertSource | null>(
    () => (location.state as { issued?: IssuedAlertSource } | null)?.issued ?? null,
  );

  // The token came with the navigation that made the source: once read, it leaves the history.
  React.useEffect(() => {
    if ((location.state as { issued?: unknown } | null)?.issued) {
      navigate(location.pathname, { replace: true, state: null });
    }
  }, [location.pathname, location.state, navigate]);

  const query = useQuery({
    queryKey: [...ALERT_SOURCES_KEY, id],
    queryFn: () => alertSourcesApi.get(id),
    enabled: !!id,
  });
  const requestsQuery = useQuery({
    queryKey: [...ALERT_SOURCES_KEY, id, 'requests', REQUEST_PAGE],
    queryFn: () => alertSourcesApi.requests(id, REQUEST_PAGE),
    enabled: !!id,
    refetchInterval: 15_000,
  });
  // The rules page's query: the incidents still open, for the ones this source opened.
  const incidentsQuery = useQuery({
    queryKey: ['alerts', 'incidents', 'active'],
    queryFn: () => incidentsApi.list(),
    refetchInterval: 30_000,
  });
  const clustersQuery = useQuery({
    queryKey: [...ALERT_SOURCES_KEY, 'clusters'],
    queryFn: () => alertSourcesApi.clusters(),
    staleTime: 60_000,
  });
  const instanceQuery = useQuery({ queryKey: ['instance'], queryFn: instanceApi.get, staleTime: 300_000 });
  const origin = resolvePublicBaseUrl(instanceQuery.data?.externalUrl, window.location.origin);

  const rotate = useMutation({
    mutationFn: () => alertSourcesApi.rotateToken(id),
    onSuccess: (source) => {
      toast.success(t('alert_sources.token.rotated'));
      setRotating(false);
      setIssued(source);
      void queryClient.invalidateQueries({ queryKey: ALERT_SOURCES_KEY });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const remove = useMutation({
    mutationFn: () => alertSourcesApi.remove(id),
    onSuccess: () => {
      toast.success(t('alert_sources.toast_deleted'));
      void queryClient.invalidateQueries({ queryKey: ALERT_SOURCES_KEY });
      navigate('/alerts/sources');
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const notFound = query.isError && toApiError(query.error).status === 404;
  const source = notFound ? null : query.data ?? null;
  const queryState = queryStateFor({ isLoading: query.isLoading, isError: query.isError, data: source });
  const pageState: ProductStateProps | null = notFound
    ? {
        variant: 'empty',
        title: t('alert_sources.detail.not_found_title'),
        description: t('alert_sources.detail.not_found_description'),
      }
    : productStateFor(queryState, { error: query.error });

  const local = clustersQuery.data?.local ?? '';
  const elsewhere = Boolean(source && local && source.homeCluster && source.homeCluster !== local);
  const address = source ? intakeAddress(origin, source) : '';
  // Where the token goes is said once: in the setup, or in the address when that is the whole setup.
  const setup = source ? setupSnippet(source.kind, address) : null;
  const requests = requestsQuery.data ?? [];
  const docsLocale = i18n.language.toLowerCase().startsWith('zh') ? 'zh-Hans' : 'en-US';
  // The steps are checked once the sender has evidently been set up: a request came through.
  const settled = source ? ['receiving', 'quiet'].includes(sourceState(source, requests).state) : false;
  const stepState = settled ? 'done' : 'todo';

  return (
    <>
      <DetailPage
        appearance="surface"
        backTo="/alerts/sources"
        title={source?.name ?? t('alert_sources.detail.loading')}
        subtitle={source ? t(`alert_sources.kinds.${source.kind}`) : undefined}
        headerIcon={source ? SOURCE_KIND_ICONS[source.kind] : null}
        state={pageState}
        toolbar={
          source ? (
            <>
              <ChromeButton
                disabled={manageAccess.disabled}
                disabledReason={manageAccess.reason}
                onClick={() => setRemoving(true)}
                data-testid="alert-source-delete"
              >
                <Trash2 className="h-3.5 w-3.5" />
                {tc('actions.delete')}
              </ChromeButton>
              <ChromeButton
                disabled={manageAccess.disabled}
                disabledReason={manageAccess.reason}
                onClick={() => setEditing(true)}
              >
                <Pencil className="h-3.5 w-3.5" />
                {tc('actions.edit')}
              </ChromeButton>
            </>
          ) : undefined
        }
      >
        {source && (
          <>
            <SourceStanding
              source={source}
              requests={requests}
              requestLimit={REQUEST_PAGE}
              incidents={incidentsQuery.data ?? []}
              policyName={policyName(source)}
              nowMicros={Date.now() * 1000}
            />

            {source.kind === 'email' && <MailboxSection source={source} elsewhere={elsewhere} />}

            <SourceSection
              title={t(source.kind === 'email' ? 'alert_sources.detail.intake_email' : 'alert_sources.detail.intake')}
              action={
                <a
                  href={`https://docs.molesignal.com/${docsLocale}/alerting/sources#${t(`alert_sources.setup_docs_anchor.${source.kind}`)}`}
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex shrink-0 items-center gap-1 rounded font-sans text-sm font-strong text-tx-2 outline-none transition-colors hover:text-tx-0 focus-visible:text-tx-0 focus-visible:underline"
                >
                  {t('alert_sources.detail.setup_docs')}
                  <ArrowUpRight aria-hidden className="h-3.5 w-3.5" />
                </a>
              }
            >
              <ol className={cn('m-0 list-none pb-[20px] pt-1', SECTION_INSET)}>
                <SetupStep
                  number={1}
                  state={stepState}
                  title={t('alert_sources.steps.address')}
                  description={t('alert_sources.steps.address_description')}
                >
                  <CopyableValue
                    value={address}
                    copyLabel={t('alert_sources.detail.copy_address')}
                    copiedLabel={tc('actions.copied')}
                  />
                  {!setup && <ValueHint>{t('alert_sources.detail.setup_hint')}</ValueHint>}
                </SetupStep>
                <SetupStep
                  number={2}
                  state={stepState}
                  title={t('alert_sources.steps.token')}
                  description={t(tokenInAddress(source.kind) ? 'alert_sources.steps.token_query' : 'alert_sources.steps.token_header')}
                >
                  <div className="flex min-w-0 items-center gap-2">
                    <div className="min-w-0 flex-1">
                      <ReadOnlyValue value={`${source.tokenPrefix}…`} mono />
                    </div>
                    <ChromeButton
                      disabled={manageAccess.disabled}
                      disabledReason={manageAccess.reason}
                      onClick={() => setRotating(true)}
                      data-testid="alert-source-rotate"
                    >
                      <KeyRound className="h-3.5 w-3.5" />
                      {t('alert_sources.token.rotate')}
                    </ChromeButton>
                  </div>
                  <ValueHint>{t('alert_sources.detail.token_hint')}</ValueHint>
                </SetupStep>
                <SetupStep
                  number={3}
                  state={stepState}
                  title={t('alert_sources.steps.configure')}
                  description={t(`alert_sources.setup_guide.${source.kind}`)}
                >
                  {setup && (
                    <>
                      <SnippetBlock content={setup.content} label={t('alert_sources.detail.copy_setup')} />
                      <ValueHint>{t('alert_sources.detail.setup_hint')}</ValueHint>
                    </>
                  )}
                </SetupStep>
              </ol>
            </SourceSection>

            {/* With nothing received the opening says so; another cluster's records are named. */}
            {(requests.length > 0 || elsewhere) && (
              <SourceSection
                title={t('alert_sources.detail.requests')}
                description={
                  elsewhere
                    ? t('alert_sources.detail.home_elsewhere', { cluster: source.homeCluster })
                    : t('alert_sources.detail.requests_description')
                }
              >
                {requests.length > 0 && <SourceRequests requests={requests.slice(0, LISTED_REQUESTS)} />}
              </SourceSection>
            )}
          </>
        )}
      </DetailPage>
      <SourceDrawer
        open={editing}
        editing={source}
        onClose={() => setEditing(false)}
        onCreated={() => undefined}
      />
      <SourceTokenDialog issued={issued} origin={origin} onClose={() => setIssued(null)} />
      <ConfirmDialog
        open={rotating}
        onOpenChange={(open) => !open && setRotating(false)}
        title={t('alert_sources.token.rotate_title')}
        description={t('alert_sources.token.rotate_description')}
        confirmLabel={t('alert_sources.token.rotate')}
        busy={rotate.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (manageAccess.allowed) rotate.mutate();
        }}
      />
      <ConfirmDialog
        open={removing}
        onOpenChange={(open) => !open && setRemoving(false)}
        destructive
        title={t('alert_sources.delete_title')}
        description={t('alert_sources.delete_description', { name: source?.name ?? '' })}
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (manageAccess.allowed) remove.mutate();
        }}
      />
    </>
  );
}
