import { LockKeyhole, Plus, RefreshCcw, Search, SlidersHorizontal } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { FilterArea, KpiStrip, MetadataStrip } from '@/admin';
import { ChromeButton, DataTable, Td, Th, Tr } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';

import { BuilderPage, DetailPage, GatePage, ListPage, OverviewPage, SettingsPage } from './templates';

type TemplateKind = 'overview' | 'list' | 'detail' | 'builder' | 'settings' | 'gate';

const TEMPLATE_KINDS: readonly TemplateKind[] = ['overview', 'list', 'detail', 'builder', 'settings', 'gate'];

export function ProductTemplateFixture() {
  const { t } = useTranslation(['product', 'design-system']);
  const [kind, setKind] = React.useState<TemplateKind>('overview');
  const toolbar = (
    <>
      <TemplateTabs active={kind} onSelect={setKind} />
      <ChromeButton>
        <RefreshCcw className="h-3 w-3" />
        {t('product:actions.refresh')}
      </ChromeButton>
    </>
  );

  if (kind === 'list') {
    return (
      <ListPage
        title={t('product:templates.list')}
        subtitle={t('product:templates.fixture_subtitle')}
        toolbar={toolbar}
        kpis={sampleKpis(t)}
        filters={<SampleFilters />}
        actionBar={<SampleActionBar />}
      >
        <SampleTable />
      </ListPage>
    );
  }

  if (kind === 'detail') {
    return (
      <DetailPage
        title={t('product:templates.detail')}
        subtitle={t('product:templates.fixture_subtitle')}
        toolbar={toolbar}
        metadata={[
          { label: t('product:fixture.owner'), value: 'telemetry' },
          { label: t('product:fixture.updated'), value: '2m ago' },
          { label: t('product:fixture.state'), value: t('product:fixture.healthy') },
        ]}
      >
        <SampleDetail />
      </DetailPage>
    );
  }

  if (kind === 'builder') {
    return (
      <BuilderPage
        title={t('product:templates.builder')}
        subtitle={t('product:templates.fixture_subtitle')}
        toolbar={toolbar}
        palette={<SamplePalette />}
        inspector={<SampleInspector />}
      >
        <SampleBuilderCanvas />
      </BuilderPage>
    );
  }

  if (kind === 'settings') {
    return (
      <SettingsPage
        title={t('product:templates.settings')}
        subtitle={t('product:templates.fixture_subtitle')}
        toolbar={toolbar}
        sections={<SampleSettingsNav />}
      >
        <SampleSettingsPanel />
      </SettingsPage>
    );
  }

  if (kind === 'gate') {
    return (
      <GatePage
        title={t('product:templates.gate')}
        subtitle={t('product:templates.fixture_subtitle')}
        toolbar={toolbar}
        state={{
          variant: 'license-gated',
          action: (
            <ChromeButton variant="primary">
              <LockKeyhole className="h-3 w-3" />
              {t('product:actions.review_license')}
            </ChromeButton>
          ),
        }}
      />
    );
  }

  return (
    <OverviewPage
      title={t('product:templates.overview')}
      subtitle={t('product:templates.fixture_subtitle')}
      toolbar={toolbar}
      kpis={sampleKpis(t)}
      aside={<SampleAside />}
    >
      <SampleOverview />
    </OverviewPage>
  );
}

function TemplateTabs({
  active,
  onSelect,
}: {
  active: TemplateKind;
  onSelect: (kind: TemplateKind) => void;
}) {
  const { t } = useTranslation('product');
  return (
    <div className="flex h-[26px] items-center rounded-md border border-bd-1 bg-bg-2 p-0.5">
      {TEMPLATE_KINDS.map((kind) => (
        <button
          key={kind}
          type="button"
          onClick={() => onSelect(kind)}
          className={cn(
            'h-5 rounded px-2 font-sans text-xs font-bold text-tx-2 hover:bg-bg-3 hover:text-tx-0',
            active === kind && 'bg-bg-4 text-tx-0',
          )}
        >
          {t(`templates.${kind}`)}
        </button>
      ))}
    </div>
  );
}

function SampleFilters() {
  const { t } = useTranslation('product');
  return (
    <>
      <ChromeButton>
        <Search className="h-3 w-3" />
        {t('product:fixture.search')}
      </ChromeButton>
      <ChromeButton>
        <SlidersHorizontal className="h-3 w-3" />
        {t('product:fixture.filters')}
      </ChromeButton>
    </>
  );
}

function SampleActionBar() {
  const { t } = useTranslation('product');
  return (
    <>
      <span className="font-sans text-xs text-tx-2">{t('product:fixture.selected', { count: 3 })}</span>
      <ChromeButton variant="primary">
        <Plus className="h-3 w-3" />
        {t('product:actions.create')}
      </ChromeButton>
    </>
  );
}

function SampleTable() {
  const { t } = useTranslation('product');
  return (
    <DataTable>
      <thead>
        <tr>
          <Th>{t('product:fixture.name')}</Th>
          <Th>{t('product:fixture.owner')}</Th>
          <Th>{t('product:fixture.state')}</Th>
          <Th>{t('product:fixture.updated')}</Th>
        </tr>
      </thead>
      <tbody>
        {['datasource', 'dashboards', 'alerts'].map((name) => (
          <Tr key={name}>
            <Td>{name}</Td>
            <Td>platform</Td>
            <Td>{t('product:fixture.healthy')}</Td>
            <Td>2m ago</Td>
          </Tr>
        ))}
      </tbody>
    </DataTable>
  );
}

function SampleDetail() {
  return (
    <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_320px]">
      <section className="min-h-80 rounded-md border border-bd-0 bg-bg-1 p-4">
        <div className="h-full rounded border border-dashed border-bd-1 bg-bg-0" />
      </section>
      <section className="rounded-md border border-bd-0 bg-bg-1 p-4">
        <MetadataStrip
          items={[
            { label: 'p95', value: '182ms' },
            { label: 'errors', value: '0.12%' },
            { label: 'runs', value: '48' },
          ]}
          className="border"
        />
      </section>
    </div>
  );
}

function SampleBuilderCanvas() {
  return (
    <div className="grid min-h-[420px] place-items-center rounded-md border border-dashed border-bd-1 bg-bg-1">
      <div className="h-36 w-64 rounded-md border border-bd-0 bg-bg-2" />
    </div>
  );
}

function SamplePalette() {
  return <FilterArea>logs metrics traces alerts</FilterArea>;
}

function SampleInspector() {
  return <MetadataStrip items={[{ label: 'mode', value: 'builder' }, { label: 'state', value: 'draft' }]} />;
}

function SampleSettingsNav() {
  const { t } = useTranslation('product');
  return (
    <nav className="rounded-md border border-bd-0 bg-bg-1 p-2">
      {['general', 'organization', 'license'].map((key) => (
        <button key={key} type="button" className="block h-7 w-full rounded px-2 text-left font-sans text-xs text-tx-1 hover:bg-bg-3">
          {t(`fixture.${key}`)}
        </button>
      ))}
    </nav>
  );
}

function SampleSettingsPanel() {
  return (
    <section className="min-h-80 rounded-md border border-bd-0 bg-bg-1 p-4">
      <MetadataStrip items={[{ label: 'region', value: 'us-east-1' }, { label: 'mode', value: 'self-hosted' }]} className="border" />
    </section>
  );
}

function SampleAside() {
  return (
    <section className="rounded-md border border-bd-0 bg-bg-1 p-3">
      <KpiStrip
        items={[
          { label: 'SLO', value: '99.95%' },
          { label: 'MTTR', value: '11m' },
        ]}
        className="grid-cols-1"
      />
    </section>
  );
}

function SampleOverview() {
  return (
    <section className="min-h-96 rounded-md border border-bd-0 bg-bg-1 p-4">
      <div className="grid h-full grid-cols-3 gap-2">
        <div className="rounded bg-bg-2" />
        <div className="rounded bg-bg-2" />
        <div className="rounded bg-bg-2" />
      </div>
    </section>
  );
}

function sampleKpis(t: (key: string) => string) {
  return [
    { label: t('product:fixture.datasource'), value: '2.4 TB', sub: t('product:fixture.last_24h') },
    { label: t('product:fixture.latency'), value: '182 ms', sub: 'p95', tone: 'good' },
    { label: t('product:fixture.errors'), value: '0.12%', sub: t('product:fixture.last_24h'), tone: 'warn' },
    { label: t('product:fixture.cost'), value: '$842', sub: t('product:fixture.month_to_date') },
  ] as const;
}
