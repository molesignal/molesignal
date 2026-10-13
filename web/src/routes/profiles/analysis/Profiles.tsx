import { useQuery } from '@tanstack/react-query';
import { GitCompare, Upload, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate, useSearchParams } from 'react-router-dom';

import * as profilesApi from '@/api/profiles';
import type { ProfileTypeName } from '@/api/profiles';
import { useActionAccess } from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ListPage } from '@/product/templates';
import { ChromeButton, TimeRangeChip } from '@/shell/chrome';
import { QueryState } from '@/shell/query/State';
import { formatDuration, formatValue, topFunctions } from '@/viz/profiles/flamebearer';
import { Flamegraph } from '@/viz/profiles/Flamegraph';

import {
  compareHref,
  profileTypesIn,
  TruncatedNotice,
  useProfileTypeLabel,
  useWindowMicros,
} from '../shared';
import { UploadProfileDialog } from '../UploadDialog';
import { ProfileActivation } from './Activation';
import { ProfileFilterBar } from './FilterBar';
import { ProfileSelectionDrawer } from './SelectionDrawer';
import { ProfileSummary } from './Summary';

const LIVE_INTERVAL_MS = 10_000;
/** As many profiles as one request lists. A window with fewer has been listed whole. */
const LIST_LIMIT = 200;

export function Profiles() {
  const { t } = useTranslation('profiles');
  const navigate = useNavigate();
  const uploadAccess = useActionAccess({ permission: 'streams.write' });
  const typeLabel = useProfileTypeLabel();
  const { window: timeWindow, fromMicros, toMicros } = useWindowMicros();
  const [searchParams] = useSearchParams();
  const traceId = searchParams.get('trace_id') ?? undefined;
  const spanId = searchParams.get('span_id') ?? undefined;
  const [service, setService] = React.useState('');
  // The type the reader picked. Until they pick one, the first type the window has profiles of.
  const [chosenType, setChosenType] = React.useState('');
  const [label, setLabel] = React.useState('');
  const [uploadOpen, setUploadOpen] = React.useState(false);
  const [selectionOpen, setSelectionOpen] = React.useState(false);
  const [live, setLive] = React.useState(false);
  const [excludedProfileIds, setExcludedProfileIds] = React.useState<Set<string>>(
    () => new Set(),
  );
  const [selectedFunction, setSelectedFunction] = React.useState<string | null>(null);

  const winKey = [timeWindow.from, timeWindow.to] as const;

  // What the window holds, of every type: it says which types there are to analyse. A flame
  // graph is of one type, since their values are not of one unit: seconds of CPU and bytes of
  // memory do not add up to anything.
  const windowQuery = useQuery({
    queryKey: ['profiles-window', ...winKey, label, traceId ?? ''],
    queryFn: () =>
      profilesApi.list({
        from: fromMicros,
        to: toMicros,
        label: label || undefined,
        trace_id: traceId,
        limit: LIST_LIMIT,
      }),
  });
  const windowRows = React.useMemo(() => windowQuery.data ?? [], [windowQuery.data]);
  const windowTypes = React.useMemo(() => profileTypesIn(windowRows), [windowRows]);
  const type = chosenType || windowTypes[0] || '';
  // A window listed whole already has the profiles of the type; a longer one is asked for them.
  const windowListedWhole = windowRows.length < LIST_LIMIT;

  const typedQuery = useQuery({
    queryKey: ['profiles-list', ...winKey, type, label, traceId ?? ''],
    queryFn: () =>
      profilesApi.list({
        from: fromMicros,
        to: toMicros,
        type: type as ProfileTypeName,
        label: label || undefined,
        trace_id: traceId,
        limit: LIST_LIMIT,
      }),
    enabled: type !== '' && !windowListedWhole,
  });
  const listQuery = windowListedWhole ? windowQuery : typedQuery;
  const rows = React.useMemo(
    () =>
      windowListedWhole
        ? windowRows.filter((row) => row.profile_type === type)
        : typedQuery.data ?? [],
    [type, typedQuery.data, windowListedWhole, windowRows],
  );
  const services = React.useMemo(
    () => Array.from(new Set(rows.map((row) => row.service).filter(Boolean))).sort(),
    [rows],
  );
  const tableRows = React.useMemo(
    () => (service ? rows.filter((row) => row.service === service) : rows),
    [rows, service],
  );
  const selectedRows = React.useMemo(
    () => tableRows.filter((row) => !excludedProfileIds.has(row.id)),
    [excludedProfileIds, tableRows],
  );
  const selectedProfileIds = React.useMemo(
    () => selectedRows.map((row) => row.id).sort(),
    [selectedRows],
  );
  const selectionKey = selectedProfileIds.join(',');
  const fallbackToTraceCorrelation = tableRows.length === 0 && Boolean(traceId);

  React.useEffect(() => {
    setExcludedProfileIds(new Set());
    setSelectedFunction(null);
  }, [label, service, spanId, timeWindow.from, timeWindow.to, traceId, type]);

  const flameQuery = useQuery({
    queryKey: [
      'profiles-flame-workbench',
      ...winKey,
      service,
      type,
      label,
      traceId ?? '',
      spanId ?? '',
      selectionKey || 'none',
    ],
    queryFn: () => {
      if (selectedProfileIds.length > 0) {
        return profilesApi.flamegraphSelection({
          profile_ids: selectedProfileIds,
          max_merge: 1000,
        });
      }
      return profilesApi.flamegraph({
        from: fromMicros,
        to: toMicros,
        service: service || undefined,
        type: (type || undefined) as ProfileTypeName | undefined,
        label: label || undefined,
        trace_id: traceId,
        span_id: spanId,
      });
    },
    enabled: selectedProfileIds.length > 0 || fallbackToTraceCorrelation,
  });

  const refetchRef = React.useRef(() => {});
  refetchRef.current = () => {
    void windowQuery.refetch();
    if (!windowListedWhole) void typedQuery.refetch();
    void flameQuery.refetch();
  };
  React.useEffect(() => {
    if (!live) return;
    const id = window.setInterval(() => refetchRef.current(), LIVE_INTERVAL_MS);
    return () => window.clearInterval(id);
  }, [live]);

  const fb = flameQuery.data?.flamebearer;
  const hotFunction = React.useMemo(
    () => (fb ? topFunctions(fb)[0] ?? null : null),
    [fb],
  );
  const totalSamples = React.useMemo(
    () => selectedRows.reduce((sum, row) => sum + (row.sample_count || 0), 0),
    [selectedRows],
  );
  const totalDuration = React.useMemo(
    () => selectedRows.reduce((sum, row) => sum + (row.duration_nanos || 0), 0),
    [selectedRows],
  );
  const selectedServices = React.useMemo(
    () => Array.from(new Set(selectedRows.map((row) => row.service).filter(Boolean))).sort(),
    [selectedRows],
  );
  const analysisService =
    service || (selectedServices.length === 1 ? selectedServices[0] : undefined);

  // Nothing of any type in the window: there is nothing to analyse yet.
  const empty =
    !windowQuery.isLoading && !windowQuery.isError && windowRows.length === 0 && !traceId;
  const listLoading = windowQuery.isLoading || listQuery.isLoading;
  const listError = windowQuery.isError ? windowQuery.error : listQuery.isError ? listQuery.error : null;
  const listState = productStateFor(listLoading ? 'loading' : listError ? 'error' : null, {
    error: listError,
  });

  const toggleProfile = React.useCallback((id: string, selected: boolean) => {
    setExcludedProfileIds((previous) => {
      const next = new Set(previous);
      if (selected) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const toggleAllProfiles = React.useCallback(
    (selected: boolean) => {
      setExcludedProfileIds((previous) => {
        const next = new Set(previous);
        for (const row of tableRows) {
          if (selected) next.delete(row.id);
          else next.add(row.id);
        }
        return next;
      });
    },
    [tableRows],
  );

  const toolbar = (
    <div className="flex items-center gap-2">
      <TimeRangeChip className="border-0" />
      {/* A way on from the analysis, not what the page is for: it is not the filled button. */}
      <ChromeButton onClick={() => navigate(compareHref({ service, type }))}>
        <GitCompare className="h-3.5 w-3.5" /> {t('compare.title')}
      </ChromeButton>
      {uploadAccess.allowed && (
        <ChromeButton onClick={() => setUploadOpen(true)}>
          <Upload className="h-3.5 w-3.5" /> {t('upload.button')}
        </ChromeButton>
      )}
    </div>
  );

  const noSelection = tableRows.length > 0 && selectedRows.length === 0;

  return (
    <>
      <ListPage
        appearance="surface"
        title={t('title')}
        subtitle={t('subtitle') as string}
        toolbar={toolbar}
        state={listState}
        // The page is several surfaces of its own: they sit on the canvas, not in one more card.
        contentSurface={false}
      >
        {empty ? (
          <ProfileActivation
            onUpload={uploadAccess.allowed ? () => setUploadOpen(true) : undefined}
          />
        ) : (
          <div className="space-y-[12px]">
            {traceId && (
              <div className="flex items-center gap-2">
                <span className="inline-flex items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 py-1 font-sans text-xs text-tx-1">
                  {t('detail.metadata.trace')}:{' '}
                  <code className="font-mono">{traceId.slice(0, 16)}</code>
                  {spanId && (
                    <>
                      · {t('detail.metadata.span')}:{' '}
                      <code className="font-mono">{spanId.slice(0, 12)}</code>
                    </>
                  )}
                  <Link
                    to="/profiles"
                    className="ml-1 text-tx-3 hover:text-tx-0"
                    aria-label={t('analysis.clear_trace')}
                  >
                    <X className="h-3 w-3" />
                  </Link>
                </span>
              </div>
            )}

            <ProfileFilterBar
              service={service}
              services={services}
              type={type}
              presentTypes={windowListedWhole ? windowTypes : undefined}
              label={label}
              live={live}
              refetching={windowQuery.isFetching || listQuery.isFetching || flameQuery.isFetching}
              onServiceChange={setService}
              onTypeChange={setChosenType}
              onLabelChange={setLabel}
              onRefresh={() => refetchRef.current()}
              onLiveChange={setLive}
            />

            <ProfileSummary
              typeLabel={typeLabel(type)}
              totalValue={fb ? formatValue(fb.numTicks, fb.units) : null}
              sampleCount={totalSamples}
              serviceCount={selectedServices.length}
              duration={selectedRows.length > 0 ? formatDuration(totalDuration) : null}
              selectedCount={selectedRows.length}
              profileCount={tableRows.length}
              hotFunction={hotFunction}
              onSelectHotFunction={(name) => setSelectedFunction(name)}
              onOpenProfiles={() => setSelectionOpen(true)}
            />

            {flameQuery.data?.truncated && <TruncatedNotice />}

            <section
              aria-label={t('analysis.workspace_aria')}
              className="overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]"
            >
              {noSelection ? (
                <QueryState
                  state="empty"
                  empty={
                    <div>
                      <div className="font-semibold text-tx-1">
                        {t('analysis.no_selection_title')}
                      </div>
                      <div className="mt-1 text-xs text-tx-3">
                        {t('analysis.no_selection_description')}
                      </div>
                      <ChromeButton size="sm" className="mt-3" onClick={() => setSelectionOpen(true)}>
                        {t('analysis.no_selection_action')}
                      </ChromeButton>
                    </div>
                  }
                  className="min-h-[360px]"
                />
              ) : flameQuery.isLoading ? (
                <QueryState
                  state="loading"
                  loadingLabel={t('analysis.loading_flamegraph')}
                  className="min-h-[360px]"
                />
              ) : flameQuery.isError ? (
                <QueryState
                  state="error"
                  error={flameQuery.error}
                  className="min-h-[360px]"
                />
              ) : fb ? (
                <Flamegraph
                  flamebearer={fb}
                  selectedFunction={selectedFunction}
                  onSelectedFunctionChange={setSelectedFunction}
                  service={analysisService}
                  onCompare={() => navigate(compareHref({ service, type }))}
                  // How many profiles are merged is said over the graph, where it is changed; a
                  // graph found by its trace has no list to say it in.
                  headerExtra={
                    tableRows.length === 0 ? (
                      <span>
                        {t('flamegraph.profiles_merged', {
                          count: flameQuery.data?.profile_count ?? 0,
                        })}
                      </span>
                    ) : undefined
                  }
                  className="rounded-none border-0"
                />
              ) : (
                <QueryState
                  state="empty"
                  emptyLabel={t('flamegraph.no_data_description')}
                  className="min-h-[360px]"
                />
              )}
            </section>

          </div>
        )}
      </ListPage>
      <ProfileSelectionDrawer
        open={selectionOpen}
        onOpenChange={setSelectionOpen}
        rows={tableRows}
        excluded={excludedProfileIds}
        onToggle={toggleProfile}
        onToggleAll={toggleAllProfiles}
        onOpenProfile={(row) => navigate(`/profiles/${encodeURIComponent(row.id)}`)}
      />
      {uploadAccess.allowed && (
        <UploadProfileDialog
          open={uploadOpen}
          onOpenChange={setUploadOpen}
          defaultService={service}
          onUploaded={() => refetchRef.current()}
        />
      )}
    </>
  );
}
