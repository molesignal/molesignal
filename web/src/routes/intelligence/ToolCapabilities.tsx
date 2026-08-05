import { useMutation, useQuery, useQueryClient, type UseQueryResult } from '@tanstack/react-query';
import {
  Activity,
  AlertTriangle,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  ClipboardCheck,
  Copy,
  Ellipsis,
  FlaskConical,
  Info,
  ListFilter,
  Network,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  Server,
  ShieldCheck,
  SlidersHorizontal,
  Unplug,
  Wrench,
  XCircle,
  Zap,
  type LucideIcon,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as intelligenceApi from '@/api/intelligence';
import { CopyIconButton } from '@/shell/CopyIconButton';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
} from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { Badge } from '@/shell/ui/badge';
import { Button } from '@/shell/ui/button';
import { Checkbox } from '@/shell/ui/checkbox';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { Input } from '@/shell/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shell/ui/tabs';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

type ToolFilter = 'all' | 'read_only' | 'automatic' | 'approval' | 'disabled';
type GroupMode = 'domain' | 'mcp' | 'none';
type McpRemovalTarget = {
  server: intelligenceApi.McpServer;
  tools: intelligenceApi.RegisteredTool[];
  dependencies: intelligenceApi.ToolDependencies[];
};

const DOMAIN_ORDER: intelligenceApi.ToolDomain[] = [
  'observability',
  'alerts_on_call',
  'automation',
  'knowledge_context',
  'dashboard_reports',
  'notify',
  'administration',
];

const RISK_ORDER: intelligenceApi.RiskLevel[] = ['l0', 'l1', 'l2', 'l3', 'l4'];
const EXECUTION_MODES: intelligenceApi.ToolExecutionMode[] = [
  'automatic',
  'confirmation',
  'single_approval',
  'dual_approval',
  'disabled',
];

const EMPTY_TOOLS: intelligenceApi.RegisteredTool[] = [];
const COLLAPSED_STORAGE_KEY = 'molesignal.intelligence.tools.collapsed.v1';

export function ToolCapabilitiesPanel({
  registry,
}: {
  registry: UseQueryResult<intelligenceApi.ToolRegistry, Error>;
}) {
  const { t } = useTranslation('intelligence');
  const queryClient = useQueryClient();
  const [search, setSearch] = React.useState('');
  const [filter, setFilter] = React.useState<ToolFilter>('all');
  const [groupMode, setGroupMode] = React.useState<GroupMode>('domain');
  const [sourceFilter, setSourceFilter] = React.useState<string[]>([]);
  const [domainFilter, setDomainFilter] = React.useState<string[]>([]);
  const [riskFilter, setRiskFilter] = React.useState<string[]>([]);
  const [selectedTool, setSelectedTool] =
    React.useState<intelligenceApi.RegisteredTool | null>(null);
  const [policyTool, setPolicyTool] =
    React.useState<intelligenceApi.RegisteredTool | null>(null);
  const [testTool, setTestTool] =
    React.useState<intelligenceApi.RegisteredTool | null>(null);
  const [dependencyTool, setDependencyTool] =
    React.useState<intelligenceApi.RegisteredTool | null>(null);
  const [pendingDisable, setPendingDisable] = React.useState<{
    tool: intelligenceApi.RegisteredTool;
    dependencies: intelligenceApi.ToolDependencies;
  } | null>(null);
  const [policyDefaultsOpen, setPolicyDefaultsOpen] = React.useState(false);
  const [mcpOpen, setMcpOpen] = React.useState(false);
  const [collapsed, setCollapsed] = React.useState<Set<string>>(() => readCollapsedGroups());

  const tools = React.useMemo(
    () => (registry.data?.tools ?? EMPTY_TOOLS).map(normalizeTool),
    [registry.data?.tools],
  );
  const filteredTools = React.useMemo(() => {
    const needle = search.trim().toLocaleLowerCase();
    return tools.filter((tool) => {
      const matchesSearch =
        !needle ||
        [
          tool.name,
          tool.display_name,
          tool.description,
          tool.technical_description,
          tool.domain,
          tool.category,
          tool.source.server_name,
          tool.source.label,
          ...tool.tags,
        ]
          .filter(Boolean)
          .some((value) => String(value).toLocaleLowerCase().includes(needle));
      if (!matchesSearch) return false;
      if (sourceFilter.length > 0 && !sourceFilter.includes(tool.source.kind)) return false;
      if (domainFilter.length > 0 && !domainFilter.includes(tool.domain)) return false;
      if (riskFilter.length > 0 && !riskFilter.includes(tool.risk)) return false;
      if (filter === 'read_only' && !tool.capabilities.read_only) return false;
      if (filter === 'automatic' && tool.execution_mode !== 'automatic') return false;
      if (
        filter === 'approval' &&
        !['confirmation', 'single_approval', 'dual_approval'].includes(tool.execution_mode)
      ) {
        return false;
      }
      if (filter === 'disabled' && tool.enabled) return false;
      return true;
    });
  }, [domainFilter, filter, riskFilter, search, sourceFilter, tools]);

  const rawGroups = React.useMemo(
    () => groupTools(filteredTools, groupMode),
    [filteredTools, groupMode],
  );
  const groups = useLocalizedGroups(rawGroups);
  const enabledCount = tools.filter((tool) => tool.enabled).length;
  const automaticCount = tools.filter(
    (tool) => tool.enabled && tool.execution_mode === 'automatic',
  ).length;
  const approvalCount = tools.filter(
    (tool) =>
      tool.enabled &&
      ['confirmation', 'single_approval', 'dual_approval'].includes(tool.execution_mode),
  ).length;
  const mcpSummary = registry.data?.mcp_servers ?? { total: 0, healthy: 0, unhealthy: 0 };

  const refresh = React.useCallback(async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ['intelligence', 'tools'] }),
      queryClient.invalidateQueries({ queryKey: ['intelligence', 'mcp-servers'] }),
      queryClient.invalidateQueries({ queryKey: ['intelligence', 'profiles'] }),
    ]);
  }, [queryClient]);

  const enableMutation = useMutation({
    mutationFn: (tool: intelligenceApi.RegisteredTool) =>
      intelligenceApi.enableTool(tool.id || tool.name),
    onSuccess: async () => {
      toast.success(t('settings.tools.feedback.enabled'));
      await refresh();
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  const disableMutation = useMutation({
    mutationFn: ({
      tool,
      force,
    }: {
      tool: intelligenceApi.RegisteredTool;
      force: boolean;
    }) => intelligenceApi.disableTool(tool.id || tool.name, force),
    onSuccess: async () => {
      setPendingDisable(null);
      toast.success(t('settings.tools.feedback.disabled'));
      await refresh();
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  const changeEnabled = React.useCallback(
    async (tool: intelligenceApi.RegisteredTool, next: boolean) => {
      if (next) {
        enableMutation.mutate(tool);
        return;
      }
      try {
        const dependencies = await intelligenceApi.getToolDependencies(tool.id || tool.name);
        if (dependencies.total > 0) {
          setPendingDisable({ tool, dependencies });
        } else {
          disableMutation.mutate({ tool, force: false });
        }
      } catch (error) {
        toast.error(errorMessage(error));
      }
    },
    [disableMutation, enableMutation],
  );

  const toggleGroup = (key: string) => {
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      writeCollapsedGroups(next);
      return next;
    });
  };

  if (registry.isLoading) return <ToolsLoading />;
  if (registry.isError) {
    return (
      <ToolsError
        onRetry={() =>
          queryClient.invalidateQueries({ queryKey: ['intelligence', 'tools'] })
        }
      />
    );
  }

  return (
    <TooltipProvider delayDuration={200}>
      <section aria-labelledby="tool-capabilities-title" className="min-w-0">
        <div className="mb-4 flex flex-wrap items-start gap-4">
          <div className="min-w-0 flex-1">
            <h2 id="tool-capabilities-title" className="text-base font-display-strong text-tx-0">
              {t('settings.tools.title')}
            </h2>
            <p className="mt-1 max-w-3xl text-xs leading-5 text-tx-3">
              {t('settings.tools.description')}
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Button size="sm" onClick={() => setPolicyDefaultsOpen(true)}>
              <SlidersHorizontal className="h-3.5 w-3.5" />
              {t('settings.tools.actions.configure_policy')}
            </Button>
            <Button size="sm" variant="outline" onClick={() => setMcpOpen(true)}>
              <Plus className="h-3.5 w-3.5" />
              {t('settings.tools.actions.add_mcp')}
            </Button>
          </div>
        </div>

        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          <SummaryCard
            icon={ShieldCheck}
            label={t('settings.tools.summary.enabled')}
            value={enabledCount}
            hint={t('settings.tools.summary.enabled_hint')}
            active={filter === 'all' && sourceFilter.length === 0}
            onClick={() => {
              setFilter('all');
              setSourceFilter([]);
            }}
          />
          <SummaryCard
            icon={Zap}
            label={t('settings.tools.summary.automatic')}
            value={automaticCount}
            hint={t('settings.tools.summary.automatic_hint')}
            active={filter === 'automatic'}
            onClick={() => setFilter('automatic')}
          />
          <SummaryCard
            icon={ClipboardCheck}
            label={t('settings.tools.summary.approval')}
            value={approvalCount}
            hint={t('settings.tools.summary.approval_hint')}
            active={filter === 'approval'}
            onClick={() => setFilter('approval')}
          />
          <SummaryCard
            icon={Server}
            label={t('settings.tools.summary.mcp')}
            value={mcpSummary.total}
            hint={
              mcpSummary.unhealthy > 0
                ? t('settings.tools.summary.mcp_unhealthy', {
                    healthy: mcpSummary.healthy,
                    unhealthy: mcpSummary.unhealthy,
                  })
                : t('settings.tools.summary.mcp_healthy', {
                    count: mcpSummary.healthy,
                  })
            }
            warning={mcpSummary.unhealthy > 0}
            onClick={() => setMcpOpen(true)}
          />
        </div>

        <div className="mt-4 flex flex-col gap-3 rounded-lg border border-bd-0 bg-bg-1 p-3 xl:flex-row xl:items-center">
          <div className="relative min-w-0 flex-1 xl:max-w-md">
            <Search
              aria-hidden
              className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-tx-3"
            />
            <Input
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={t('settings.tools.search_placeholder')}
              aria-label={t('settings.tools.search_label')}
              className="h-9 pl-9"
            />
          </div>
          <div className="flex min-w-0 flex-wrap items-center gap-1">
            {(['all', 'read_only', 'automatic', 'approval', 'disabled'] as ToolFilter[]).map(
              (value) => (
                <Button
                  key={value}
                  size="sm"
                  variant={filter === value ? 'default' : 'ghost'}
                  className="h-8 px-3 text-xs"
                  onClick={() => setFilter(value)}
                >
                  {t(`settings.tools.filters.${value}`)}
                </Button>
              ),
            )}
          </div>
          <AdvancedFilters
            sourceFilter={sourceFilter}
            domainFilter={domainFilter}
            riskFilter={riskFilter}
            onSourceChange={setSourceFilter}
            onDomainChange={setDomainFilter}
            onRiskChange={setRiskFilter}
          />
          <Select value={groupMode} onValueChange={(value) => setGroupMode(value as GroupMode)}>
            <SelectTrigger
              className="h-8 w-full bg-bg-2 text-xs xl:w-[168px]"
              aria-label={t('settings.tools.grouping.label')}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(['domain', 'mcp', 'none'] as GroupMode[]).map((value) => (
                <SelectItem key={value} value={value}>
                  {t(`settings.tools.grouping.${value}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        {tools.length === 0 ? (
          <ToolsEmpty onAddMcp={() => setMcpOpen(true)} />
        ) : filteredTools.length === 0 ? (
          <div className="mt-4 rounded-lg border border-dashed border-bd-1 bg-bg-1 px-6 py-12 text-center">
            <Search className="mx-auto h-5 w-5 text-tx-3" />
            <div className="mt-3 text-sm font-strong text-tx-1">
              {t('settings.tools.empty_filter_title')}
            </div>
            <p className="mt-1 text-xs text-tx-3">
              {t('settings.tools.empty_filter_description')}
            </p>
          </div>
        ) : (
          <div className="mt-4 space-y-3">
            {groups.map((group) => (
              <ToolGroup
                key={group.key}
                group={group}
                collapsed={collapsed.has(group.key)}
                onToggle={() => toggleGroup(group.key)}
                onDetail={setSelectedTool}
                onTest={setTestTool}
                onPolicy={setPolicyTool}
                onDependencies={setDependencyTool}
                onCalls={setSelectedTool}
                onMcp={() => setMcpOpen(true)}
                onEnabledChange={changeEnabled}
                pending={
                  enableMutation.isPending || disableMutation.isPending
                }
              />
            ))}
          </div>
        )}
      </section>

      <ToolDetailDrawer
        tool={selectedTool}
        onClose={() => setSelectedTool(null)}
        onTest={(tool) => {
          setSelectedTool(null);
          setTestTool(tool);
        }}
        onPolicy={(tool) => {
          setSelectedTool(null);
          setPolicyTool(tool);
        }}
      />
      <ToolPolicyDrawer
        tool={policyTool}
        onClose={() => setPolicyTool(null)}
        onSaved={refresh}
      />
      <ToolTestDrawer tool={testTool} onClose={() => setTestTool(null)} />
      <DependenciesDrawer
        tool={dependencyTool}
        onClose={() => setDependencyTool(null)}
      />
      <DisableConfirmationDrawer
        target={pendingDisable}
        pending={disableMutation.isPending}
        onClose={() => setPendingDisable(null)}
        onConfirm={() => {
          if (pendingDisable) {
            disableMutation.mutate({ tool: pendingDisable.tool, force: true });
          }
        }}
      />
      <PolicyDefaultsDrawer
        open={policyDefaultsOpen}
        onClose={() => setPolicyDefaultsOpen(false)}
        onSaved={refresh}
      />
      <McpServersDrawer
        open={mcpOpen}
        onClose={() => setMcpOpen(false)}
        onChanged={refresh}
      />
    </TooltipProvider>
  );
}

function SummaryCard({
  icon: Icon,
  label,
  value,
  hint,
  active = false,
  warning = false,
  onClick,
}: {
  icon: LucideIcon;
  label: string;
  value: number;
  hint: string;
  active?: boolean;
  warning?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'group flex min-h-[92px] items-center gap-3 rounded-lg border bg-bg-1 px-4 py-3 text-left transition-colors',
        active ? 'border-indigo/50 bg-indigo/5' : 'border-bd-0 hover:border-bd-1 hover:bg-bg-2',
      )}
    >
      <span
        className={cn(
          'grid h-10 w-10 shrink-0 place-items-center rounded-md border border-indigo/20 bg-indigo/10 text-indigo',
          warning && 'border-yellow/30 bg-yellow/10 text-yellow-soft',
        )}
      >
        <Icon className="h-[18px] w-[18px]" />
      </span>
      <span className="min-w-0">
        <span className="block text-xs font-strong text-tx-2">{label}</span>
        <span
          className={cn(
            'mt-0.5 block font-mono text-xl font-display-strong tabular-nums text-tx-0',
            warning && 'text-yellow-soft',
          )}
        >
          {value}
        </span>
        <span className="mt-0.5 block truncate text-xs text-tx-3">{hint}</span>
      </span>
    </button>
  );
}

function AdvancedFilters({
  sourceFilter,
  domainFilter,
  riskFilter,
  onSourceChange,
  onDomainChange,
  onRiskChange,
}: {
  sourceFilter: string[];
  domainFilter: string[];
  riskFilter: string[];
  onSourceChange: (value: string[]) => void;
  onDomainChange: (value: string[]) => void;
  onRiskChange: (value: string[]) => void;
}) {
  const { t } = useTranslation('intelligence');
  const count = sourceFilter.length + domainFilter.length + riskFilter.length;
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button size="sm" variant="outline" className="h-8 shrink-0 text-xs">
          <ListFilter className="h-3.5 w-3.5" />
          {t('settings.tools.filters.advanced')}
          {count > 0 && <Badge variant="accent" className="px-1.5 py-0">{count}</Badge>}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-64">
        <DropdownMenuLabel>{t('settings.tools.fields.source')}</DropdownMenuLabel>
        {(['builtin', 'mcp', 'custom'] as const).map((value) => (
          <DropdownMenuCheckboxItem
            key={value}
            checked={sourceFilter.includes(value)}
            onCheckedChange={() => onSourceChange(toggleListValue(sourceFilter, value))}
          >
            {t(`settings.tools.sources.${value}`)}
          </DropdownMenuCheckboxItem>
        ))}
        <DropdownMenuSeparator />
        <DropdownMenuLabel>{t('settings.tools.fields.domain')}</DropdownMenuLabel>
        {DOMAIN_ORDER.map((value) => (
          <DropdownMenuCheckboxItem
            key={value}
            checked={domainFilter.includes(value)}
            onCheckedChange={() => onDomainChange(toggleListValue(domainFilter, value))}
          >
            {t(`settings.tools.domains.${value}.title`)}
          </DropdownMenuCheckboxItem>
        ))}
        <DropdownMenuSeparator />
        <DropdownMenuLabel>{t('settings.tools.fields.risk')}</DropdownMenuLabel>
        {RISK_ORDER.map((value) => (
          <DropdownMenuCheckboxItem
            key={value}
            checked={riskFilter.includes(value)}
            onCheckedChange={() => onRiskChange(toggleListValue(riskFilter, value))}
          >
            {value.toUpperCase()} · {t(`settings.tools.risk.${value}.title`)}
          </DropdownMenuCheckboxItem>
        ))}
        {count > 0 && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              onSelect={() => {
                onSourceChange([]);
                onDomainChange([]);
                onRiskChange([]);
              }}
            >
              {t('settings.tools.filters.clear')}
            </DropdownMenuItem>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

interface ToolGroupModel {
  key: string;
  title: string;
  description: string;
  tools: intelligenceApi.RegisteredTool[];
}

function ToolGroup({
  group,
  collapsed,
  onToggle,
  onDetail,
  onTest,
  onPolicy,
  onDependencies,
  onCalls,
  onMcp,
  onEnabledChange,
  pending,
}: {
  group: ToolGroupModel;
  collapsed: boolean;
  onToggle: () => void;
  onDetail: (tool: intelligenceApi.RegisteredTool) => void;
  onTest: (tool: intelligenceApi.RegisteredTool) => void;
  onPolicy: (tool: intelligenceApi.RegisteredTool) => void;
  onDependencies: (tool: intelligenceApi.RegisteredTool) => void;
  onCalls: (tool: intelligenceApi.RegisteredTool) => void;
  onMcp: (tool: intelligenceApi.RegisteredTool) => void;
  onEnabledChange: (tool: intelligenceApi.RegisteredTool, next: boolean) => void;
  pending: boolean;
}) {
  const { t } = useTranslation('intelligence');
  return (
    <section className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
      <button
        type="button"
        className="flex min-h-11 w-full items-center gap-2 border-b border-bd-0 px-3 py-2 text-left hover:bg-bg-2"
        onClick={onToggle}
        aria-expanded={!collapsed}
      >
        {collapsed ? (
          <ChevronRight className="h-3.5 w-3.5 text-tx-3" />
        ) : (
          <ChevronDown className="h-3.5 w-3.5 text-tx-3" />
        )}
        <span className="text-sm font-strong text-tx-0">{group.title}</span>
        <Badge variant="secondary" className="px-1.5 py-0 font-mono text-type-micro">
          {group.tools.length}
        </Badge>
        <span className="hidden truncate text-xs text-tx-3 md:block">{group.description}</span>
      </button>
      {!collapsed && (
        <div role="table" aria-label={group.title}>
          <div
            role="row"
            className="hidden min-h-8 grid-cols-[minmax(260px,1fr)_110px_82px_124px_60px_36px] items-center gap-3 border-b border-bd-0 bg-bg-2 px-3 text-type-micro font-strong uppercase tracking-[0.06em] text-tx-3 xl:grid"
          >
            <span role="columnheader">{t('settings.tools.fields.tool')}</span>
            <span role="columnheader">{t('settings.tools.fields.source')}</span>
            <span role="columnheader">{t('settings.tools.fields.risk')}</span>
            <span role="columnheader">{t('settings.tools.fields.execution')}</span>
            <span role="columnheader">{t('settings.tools.fields.status')}</span>
            <span role="columnheader" />
          </div>
          <div className="divide-y divide-bd-0">
            {group.tools.map((tool) => (
              <ToolRow
                key={tool.id}
                tool={tool}
                onDetail={() => onDetail(tool)}
                onTest={() => onTest(tool)}
                onPolicy={() => onPolicy(tool)}
                onDependencies={() => onDependencies(tool)}
                onCalls={() => onCalls(tool)}
                onMcp={() => onMcp(tool)}
                onEnabledChange={(next) => onEnabledChange(tool, next)}
                pending={pending}
              />
            ))}
          </div>
        </div>
      )}
    </section>
  );
}

function ToolRow({
  tool,
  onDetail,
  onTest,
  onPolicy,
  onDependencies,
  onCalls,
  onMcp,
  onEnabledChange,
  pending,
}: {
  tool: intelligenceApi.RegisteredTool;
  onDetail: () => void;
  onTest: () => void;
  onPolicy: () => void;
  onDependencies: () => void;
  onCalls: () => void;
  onMcp: () => void;
  onEnabledChange: (next: boolean) => void;
  pending: boolean;
}) {
  const { t } = useTranslation('intelligence');
  const hasWarning =
    tool.status === 'degraded' ||
    tool.status === 'unavailable' ||
    Boolean(tool.statistics.last_error);
  return (
    <div
      role="row"
      className="grid min-h-[58px] grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-3 px-3 py-2.5 hover:bg-bg-2 xl:grid-cols-[minmax(260px,1fr)_110px_82px_124px_60px_36px]"
    >
      <button type="button" className="min-w-0 text-left" onClick={onDetail}>
        <span className="flex min-w-0 items-center gap-2">
          <span className="truncate font-mono text-xs font-strong text-tx-0">{tool.name}</span>
          {hasWarning && (
            <Tooltip>
              <TooltipTrigger asChild>
                <AlertTriangle className="h-3.5 w-3.5 shrink-0 text-yellow-soft" />
              </TooltipTrigger>
              <TooltipContent>
                {tool.statistics.last_error ?? t('settings.tools.status.unavailable')}
              </TooltipContent>
            </Tooltip>
          )}
          {!tool.available_to_agent && tool.enabled && (
            <Tooltip>
              <TooltipTrigger asChild>
                <Info className="h-3.5 w-3.5 shrink-0 text-tx-3" />
              </TooltipTrigger>
              <TooltipContent>
                {t('settings.tools.profile_restricted')}
              </TooltipContent>
            </Tooltip>
          )}
        </span>
        <span className="mt-1 block truncate text-xs text-tx-3">{tool.description}</span>
      </button>
      <div className="hidden min-w-0 xl:block">
        <SourceBadge source={tool.source} onMcp={onMcp} />
      </div>
      <div className="hidden xl:block">
        <RiskBadge risk={tool.risk} />
      </div>
      <div className="hidden xl:block">
        <ExecutionBadge mode={tool.execution_mode} />
      </div>
      <div className="flex items-center justify-end">
        <Switch
          checked={tool.enabled}
          onCheckedChange={onEnabledChange}
          disabled={pending}
          aria-label={t('settings.tools.actions.toggle', { name: tool.name })}
        />
      </div>
      <ToolMenu
        tool={tool}
        onDetail={onDetail}
        onTest={onTest}
        onPolicy={onPolicy}
        onDependencies={onDependencies}
        onCalls={onCalls}
        onMcp={onMcp}
        onEnabledChange={onEnabledChange}
      />
    </div>
  );
}

function ToolMenu({
  tool,
  onDetail,
  onTest,
  onPolicy,
  onDependencies,
  onCalls,
  onMcp,
  onEnabledChange,
}: {
  tool: intelligenceApi.RegisteredTool;
  onDetail: () => void;
  onTest: () => void;
  onPolicy: () => void;
  onDependencies: () => void;
  onCalls: () => void;
  onMcp: () => void;
  onEnabledChange: (next: boolean) => void;
}) {
  const { t } = useTranslation('intelligence');
  const copyDefinition = async () => {
    await navigator.clipboard.writeText(
      JSON.stringify(
        {
          name: tool.name,
          description: tool.technical_description,
          input_schema: tool.input_schema,
          output_schema: tool.output_schema,
        },
        null,
        2,
      ),
    );
    toast.success(t('settings.tools.feedback.definition_copied'));
  };
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          className="h-8 w-8"
          aria-label={t('settings.tools.actions.more', { name: tool.name })}
        >
          <Ellipsis className="h-4 w-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-48">
        <DropdownMenuItem onSelect={onDetail}>
          <Info className="h-3.5 w-3.5" />
          {t('settings.tools.actions.details')}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={onTest}>
          <FlaskConical className="h-3.5 w-3.5" />
          {t('settings.tools.actions.test')}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={onCalls}>
          <Activity className="h-3.5 w-3.5" />
          {t('settings.tools.actions.calls')}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={onPolicy}>
          <SlidersHorizontal className="h-3.5 w-3.5" />
          {t('settings.tools.actions.edit_policy')}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={onDependencies}>
          <Network className="h-3.5 w-3.5" />
          {t('settings.tools.actions.dependencies')}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={copyDefinition}>
          <Copy className="h-3.5 w-3.5" />
          {t('settings.tools.actions.copy_definition')}
        </DropdownMenuItem>
        {tool.source.kind === 'mcp' && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={onMcp}>
              <Server className="h-3.5 w-3.5" />
              {t('settings.tools.actions.view_mcp')}
            </DropdownMenuItem>
          </>
        )}
        <DropdownMenuSeparator />
        <DropdownMenuItem
          className={tool.enabled ? 'text-red-soft' : ''}
          onSelect={() => onEnabledChange(!tool.enabled)}
        >
          {tool.enabled ? (
            <Unplug className="h-3.5 w-3.5" />
          ) : (
            <CheckCircle2 className="h-3.5 w-3.5" />
          )}
          {t(
            tool.enabled
              ? 'settings.tools.actions.disable'
              : 'settings.tools.actions.enable',
          )}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function SourceBadge({
  source,
  onMcp,
}: {
  source: intelligenceApi.ToolSource;
  onMcp: () => void;
}) {
  const { t } = useTranslation('intelligence');
  if (source.kind === 'mcp') {
    return (
      <button
        type="button"
        className="max-w-full truncate rounded-md border border-indigo/20 bg-indigo/5 px-2 py-0.5 text-type-micro text-indigo hover:border-indigo/40"
        onClick={onMcp}
      >
        {t('settings.tools.sources.mcp')} · {source.server_name ?? source.label}
      </button>
    );
  }
  return (
    <Badge variant="outline" className="max-w-full truncate text-type-micro">
      {t(`settings.tools.sources.${source.kind}`)}
    </Badge>
  );
}

function RiskBadge({ risk }: { risk: intelligenceApi.RiskLevel }) {
  const { t } = useTranslation('intelligence');
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Badge
          variant="outline"
          className={cn(
            'font-mono text-type-micro uppercase',
            risk === 'l2' && 'border-yellow/30 text-yellow-soft',
            (risk === 'l3' || risk === 'l4') && 'border-red/30 text-red-soft',
          )}
        >
          {risk}
        </Badge>
      </TooltipTrigger>
      <TooltipContent className="max-w-xs">
        <div className="font-strong">{t(`settings.tools.risk.${risk}.title`)}</div>
        <div className="mt-0.5 text-tx-2">{t(`settings.tools.risk.${risk}.description`)}</div>
      </TooltipContent>
    </Tooltip>
  );
}

function ExecutionBadge({ mode }: { mode: intelligenceApi.ToolExecutionMode }) {
  const { t } = useTranslation('intelligence');
  return (
    <Badge
      variant="outline"
      className={cn(
        'text-type-micro',
        mode === 'confirmation' && 'border-yellow/30 text-yellow-soft',
        mode === 'single_approval' && 'border-yellow/40 text-yellow-soft',
        mode === 'dual_approval' && 'border-red/30 text-red-soft',
        mode === 'disabled' && 'text-tx-3',
      )}
    >
      {t(`settings.tools.execution.${mode}`)}
    </Badge>
  );
}

function ToolDetailDrawer({
  tool,
  onClose,
  onTest,
  onPolicy,
}: {
  tool: intelligenceApi.RegisteredTool | null;
  onClose: () => void;
  onTest: (tool: intelligenceApi.RegisteredTool) => void;
  onPolicy: (tool: intelligenceApi.RegisteredTool) => void;
}) {
  const { t } = useTranslation('intelligence');
  const dependencies = useQuery({
    queryKey: ['intelligence', 'tool-dependencies', tool?.id],
    queryFn: () => intelligenceApi.getToolDependencies(tool?.id ?? ''),
    enabled: Boolean(tool),
    retry: false,
  });
  const calls = useQuery({
    queryKey: ['intelligence', 'tool-calls', tool?.id],
    queryFn: () => intelligenceApi.listToolCalls(tool?.id ?? '', 50),
    enabled: Boolean(tool),
    retry: false,
  });
  if (!tool) return null;
  return (
    <FormDrawer
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={tool.name}
      subtitle={tool.description}
      width={720}
      footer={
        <>
          <Button variant="outline" onClick={() => onTest(tool)}>
            <FlaskConical className="h-3.5 w-3.5" />
            {t('settings.tools.actions.test')}
          </Button>
          <Button onClick={() => onPolicy(tool)}>
            <SlidersHorizontal className="h-3.5 w-3.5" />
            {t('settings.tools.actions.edit_policy')}
          </Button>
        </>
      }
    >
      <Tabs defaultValue="overview">
        <TabsList className="w-full justify-start overflow-x-auto">
          {(['overview', 'schema', 'policy', 'calls', 'dependencies'] as const).map((tab) => (
            <TabsTrigger key={tab} value={tab}>
              {t(`settings.tools.detail.tabs.${tab}`)}
            </TabsTrigger>
          ))}
        </TabsList>
        <TabsContent value="overview" className="mt-5">
          <FormSection title={t('settings.tools.detail.basic')}>
            <DetailGrid
              rows={[
                [t('settings.tools.fields.display_name'), tool.display_name],
                [
                  t('settings.tools.fields.domain'),
                  t(`settings.tools.domains.${tool.domain}.title`),
                ],
                [
                  t('settings.tools.fields.source'),
                  tool.source.kind === 'mcp'
                    ? `${t('settings.tools.sources.mcp')} · ${tool.source.server_name ?? tool.source.label}`
                    : t(`settings.tools.sources.${tool.source.kind}`),
                ],
                [t('settings.tools.fields.status'), t(`settings.tools.status.${tool.status}`)],
              ]}
            />
          </FormSection>
          <FormSection title={t('settings.tools.detail.description')}>
            <p className="text-sm leading-6 text-tx-1">{tool.description}</p>
            {tool.technical_description !== tool.description && (
              <p className="rounded-md border border-bd-0 bg-bg-2 p-3 font-mono text-xs leading-5 text-tx-3">
                {tool.technical_description}
              </p>
            )}
          </FormSection>
          <FormSection title={t('settings.tools.detail.statistics')}>
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
              <MiniStat
                label={t('settings.tools.stats.calls_24h')}
                value={String(tool.statistics.calls_24h)}
              />
              <MiniStat
                label={t('settings.tools.stats.success_rate')}
                value={
                  tool.statistics.success_rate == null
                    ? '—'
                    : `${tool.statistics.success_rate.toFixed(1)}%`
                }
              />
              <MiniStat
                label={t('settings.tools.stats.p95')}
                value={
                  tool.statistics.p95_ms == null
                    ? '—'
                    : `${tool.statistics.p95_ms}ms`
                }
              />
              <MiniStat
                label={t('settings.tools.stats.last_error')}
                value={
                  tool.statistics.last_error
                    ? t('settings.tools.stats.has_error')
                    : t('settings.tools.stats.no_error')
                }
                warning={Boolean(tool.statistics.last_error)}
              />
            </div>
          </FormSection>
        </TabsContent>
        <TabsContent value="schema" className="mt-5">
          <SchemaSection
            title={t('settings.tools.detail.input_schema')}
            schema={tool.input_schema}
          />
          <SchemaSection
            title={t('settings.tools.detail.output_schema')}
            schema={tool.output_schema ?? {}}
          />
        </TabsContent>
        <TabsContent value="policy" className="mt-5">
          <FormSection title={t('settings.tools.detail.permissions')}>
            <div className="flex flex-wrap gap-2">
              <RiskBadge risk={tool.risk} />
              <ExecutionBadge mode={tool.execution_mode} />
              <Badge variant="outline">
                {tool.enabled
                  ? t('settings.tools.status.enabled')
                  : t('settings.tools.status.disabled')}
              </Badge>
            </div>
            <DetailGrid
              rows={[
                [
                  t('settings.tools.fields.timeout'),
                  `${Math.round(tool.limits.timeout_ms / 1000)}s`,
                ],
                [
                  t('settings.tools.fields.max_calls'),
                  String(tool.limits.max_calls_per_run),
                ],
                [
                  t('settings.tools.fields.max_response'),
                  formatBytes(tool.limits.max_response_bytes),
                ],
                [
                  t('settings.tools.fields.dry_run'),
                  t(
                    tool.capabilities.supports_dry_run
                      ? 'settings.tools.common.yes'
                      : 'settings.tools.common.no',
                  ),
                ],
              ]}
            />
          </FormSection>
        </TabsContent>
        <TabsContent value="calls" className="mt-5">
          <CallRecords calls={calls.data ?? []} loading={calls.isLoading} />
        </TabsContent>
        <TabsContent value="dependencies" className="mt-5">
          <DependencyContent
            dependencies={dependencies.data}
            loading={dependencies.isLoading}
          />
        </TabsContent>
      </Tabs>
    </FormDrawer>
  );
}

function SchemaSection({
  title,
  schema,
}: {
  title: string;
  schema: Record<string, unknown>;
}) {
  const { t } = useTranslation('intelligence');
  const [showJson, setShowJson] = React.useState(false);
  const properties = schemaProperties(schema);
  const required = new Set(Array.isArray(schema.required) ? schema.required.map(String) : []);
  const copy = async () => {
    await navigator.clipboard.writeText(JSON.stringify(schema, null, 2));
    toast.success(t('settings.tools.feedback.schema_copied'));
  };
  return (
    <FormSection title={title}>
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={() => setShowJson((value) => !value)}>
          {t(showJson ? 'settings.tools.schema.form_view' : 'settings.tools.schema.json_view')}
        </Button>
        <CopyIconButton
          onClick={copy}
          label={t('settings.tools.schema.copy')}
        />
      </div>
      {showJson ? (
        <pre className="max-h-80 overflow-auto rounded-md border border-bd-0 bg-bg-2 p-3 font-mono text-xs leading-5 text-tx-2">
          {JSON.stringify(schema, null, 2)}
        </pre>
      ) : properties.length > 0 ? (
        <div className="divide-y divide-bd-0 overflow-hidden rounded-md border border-bd-0">
          {properties.map(([name, definition]) => (
            <div
              key={name}
              className="grid grid-cols-[minmax(120px,0.7fr)_90px_minmax(0,1.3fr)] gap-3 bg-bg-2 px-3 py-2.5 text-xs"
            >
              <span className="font-mono font-strong text-tx-0">
                {name}
                {required.has(name) && <span className="ml-1 text-red">*</span>}
              </span>
              <span className="font-mono text-tx-3">
                {String(definition.type ?? 'any')}
              </span>
              <span className="text-tx-3">
                {String(definition.description ?? definition.title ?? '—')}
              </span>
            </div>
          ))}
        </div>
      ) : (
        <div className="rounded-md border border-dashed border-bd-1 px-4 py-6 text-center text-xs text-tx-3">
          {t('settings.tools.schema.no_fields')}
        </div>
      )}
    </FormSection>
  );
}

function ToolPolicyDrawer({
  tool,
  onClose,
  onSaved,
}: {
  tool: intelligenceApi.RegisteredTool | null;
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const { t } = useTranslation('intelligence');
  const [risk, setRisk] = React.useState<intelligenceApi.RiskLevel>('l0');
  const [executionMode, setExecutionMode] =
    React.useState<intelligenceApi.ToolExecutionMode>('automatic');
  const [timeoutMs, setTimeoutMs] = React.useState('30000');
  const [maxCalls, setMaxCalls] = React.useState('32');
  const [maxResponse, setMaxResponse] = React.useState('1048576');
  const [environmentModes, setEnvironmentModes] = React.useState<
    Record<string, intelligenceApi.ToolExecutionMode | ''>
  >({ development: '', staging: '', production: '' });
  React.useEffect(() => {
    if (!tool) return;
    setRisk(tool.risk);
    setExecutionMode(tool.execution_mode);
    setTimeoutMs(String(tool.limits.timeout_ms));
    setMaxCalls(String(tool.limits.max_calls_per_run));
    setMaxResponse(String(tool.limits.max_response_bytes));
    setEnvironmentModes({
      development: tool.environment_overrides.development ?? '',
      staging: tool.environment_overrides.staging ?? '',
      production: tool.environment_overrides.production ?? '',
    });
  }, [tool]);
  const mutation = useMutation({
    mutationFn: () => {
      if (!tool) throw new Error(t('settings.tools.errors.no_tool'));
      const environment_overrides = Object.fromEntries(
        Object.entries(environmentModes).filter(([, value]) => value),
      ) as Record<string, intelligenceApi.ToolExecutionMode>;
      return intelligenceApi.updateToolPolicy(tool.id || tool.name, {
        risk,
        execution_mode: executionMode,
        environment_overrides,
        timeout_ms: Number(timeoutMs),
        max_calls_per_run: Number(maxCalls),
        max_response_bytes: Number(maxResponse),
      });
    },
    onSuccess: async () => {
      toast.success(t('settings.tools.feedback.policy_saved'));
      await onSaved();
      onClose();
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  if (!tool) return null;
  const modes = allowedModes(risk);
  return (
    <FormDrawer
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={t('settings.tools.policy.title', { name: tool.name })}
      subtitle={t('settings.tools.policy.description')}
      width={640}
      footer={
        <>
          <Button variant="outline" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button onClick={() => mutation.mutate()} disabled={mutation.isPending}>
            {mutation.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </>
      }
    >
      <FormSection
        title={t('settings.tools.policy.classification')}
        description={t('settings.tools.policy.hard_floor')}
      >
        <FormRow>
          <FormField label={t('settings.tools.fields.risk')}>
            <FormSelect
              value={risk}
              onChange={(value) => {
                const next = value as intelligenceApi.RiskLevel;
                setRisk(next);
                if (!allowedModes(next).includes(executionMode)) {
                  setExecutionMode(defaultMode(next));
                }
              }}
              options={RISK_ORDER.map((value) => ({
                value,
                label: `${value.toUpperCase()} · ${t(`settings.tools.risk.${value}.title`)}`,
              }))}
              className="w-full"
            />
          </FormField>
          <FormField label={t('settings.tools.fields.execution')}>
            <FormSelect
              value={executionMode}
              onChange={(value) =>
                setExecutionMode(value as intelligenceApi.ToolExecutionMode)
              }
              options={modes.map((value) => ({
                value,
                label: t(`settings.tools.execution.${value}`),
              }))}
              className="w-full"
            />
          </FormField>
        </FormRow>
        {tool.source.kind === 'builtin' && (
          <p className="rounded-md border border-bd-0 bg-bg-2 p-3 text-xs text-tx-3">
            {t('settings.tools.policy.builtin_risk_locked')}
          </p>
        )}
      </FormSection>
      <FormSection title={t('settings.tools.policy.environment_overrides')}>
        {(['development', 'staging', 'production'] as const).map((environment) => (
          <FormField
            key={environment}
            label={t(`settings.tools.environments.${environment}`)}
          >
            <FormSelect
              value={environmentModes[environment] ?? ''}
              onChange={(value) =>
                setEnvironmentModes((current) => ({
                  ...current,
                  [environment]: value as intelligenceApi.ToolExecutionMode | '',
                }))
              }
              options={[
                { value: '', label: t('settings.tools.policy.inherit') },
                ...modes.map((value) => ({
                  value,
                  label: t(`settings.tools.execution.${value}`),
                })),
              ]}
              className="w-full"
            />
          </FormField>
        ))}
      </FormSection>
      <FormSection title={t('settings.tools.policy.limits')}>
        <FormRow>
          <FormField label={t('settings.tools.fields.timeout_ms')}>
            <FormInput
              type="number"
              min={1000}
              max={120000}
              value={timeoutMs}
              onChange={(event) => setTimeoutMs(event.target.value)}
            />
          </FormField>
          <FormField label={t('settings.tools.fields.max_calls')}>
            <FormInput
              type="number"
              min={1}
              max={256}
              value={maxCalls}
              onChange={(event) => setMaxCalls(event.target.value)}
            />
          </FormField>
        </FormRow>
        <FormField label={t('settings.tools.fields.max_response_bytes')}>
          <FormInput
            type="number"
            min={1024}
            max={16777216}
            value={maxResponse}
            onChange={(event) => setMaxResponse(event.target.value)}
          />
        </FormField>
      </FormSection>
    </FormDrawer>
  );
}

function ToolTestDrawer({
  tool,
  onClose,
}: {
  tool: intelligenceApi.RegisteredTool | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('intelligence');
  const [values, setValues] = React.useState<Record<string, unknown>>({});
  const [result, setResult] = React.useState<intelligenceApi.ToolTestResult | null>(null);
  React.useEffect(() => {
    setValues(schemaDefaults(tool?.input_schema ?? {}));
    setResult(null);
  }, [tool]);
  const mutation = useMutation({
    mutationFn: (validateOnly: boolean) => {
      if (!tool) throw new Error(t('settings.tools.errors.no_tool'));
      return intelligenceApi.testTool(tool.id || tool.name, {
        arguments: values,
        dry_run: true,
        validate_only: validateOnly,
      });
    },
    onSuccess: (next) => setResult(next),
    onError: (error) => toast.error(errorMessage(error)),
  });
  if (!tool) return null;
  return (
    <FormDrawer
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={t('settings.tools.test.title', { name: tool.name })}
      subtitle={t(
        tool.capabilities.read_only
          ? 'settings.tools.test.read_description'
          : 'settings.tools.test.write_description',
      )}
      width={680}
      footer={
        <>
          <Button
            variant="outline"
            onClick={() => mutation.mutate(true)}
            disabled={mutation.isPending}
          >
            {t('settings.tools.test.validate')}
          </Button>
          <Button onClick={() => mutation.mutate(false)} disabled={mutation.isPending}>
            <FlaskConical className="h-3.5 w-3.5" />
            {tool.capabilities.read_only
              ? t('settings.tools.test.execute')
              : t('settings.tools.test.dry_run')}
          </Button>
        </>
      }
    >
      <FormSection title={t('settings.tools.test.parameters')}>
        <SchemaForm
          schema={tool.input_schema}
          values={values}
          onChange={setValues}
        />
      </FormSection>
      {result && <TestResult result={result} />}
    </FormDrawer>
  );
}

function SchemaForm({
  schema,
  values,
  onChange,
}: {
  schema: Record<string, unknown>;
  values: Record<string, unknown>;
  onChange: (value: Record<string, unknown>) => void;
}) {
  const { t } = useTranslation('intelligence');
  const properties = schemaProperties(schema);
  const required = new Set(Array.isArray(schema.required) ? schema.required.map(String) : []);
  if (properties.length === 0) {
    return (
      <div className="rounded-md border border-dashed border-bd-1 px-4 py-6 text-center text-xs text-tx-3">
        {t('settings.tools.test.no_parameters')}
      </div>
    );
  }
  return (
    <div className="space-y-4">
      {properties.map(([name, definition]) => (
        <SchemaField
          key={name}
          name={name}
          definition={definition}
          value={values[name]}
          required={required.has(name)}
          onChange={(value) => onChange({ ...values, [name]: value })}
        />
      ))}
    </div>
  );
}

function SchemaField({
  name,
  definition,
  value,
  required,
  onChange,
}: {
  name: string;
  definition: Record<string, unknown>;
  value: unknown;
  required: boolean;
  onChange: (value: unknown) => void;
}) {
  const label = String(definition.title ?? name);
  const hint = definition.description ? String(definition.description) : undefined;
  if (definition.type === 'boolean') {
    return (
      <label className="flex items-center justify-between gap-4 rounded-md border border-bd-0 bg-bg-2 px-3 py-2.5">
        <span>
          <span className="block text-xs font-strong text-tx-1">{label}</span>
          {hint && <span className="mt-0.5 block text-xs text-tx-3">{hint}</span>}
        </span>
        <Switch checked={Boolean(value)} onCheckedChange={onChange} />
      </label>
    );
  }
  if (definition.type === 'object') {
    const objectSchema = definition as Record<string, unknown>;
    const objectValue =
      typeof value === 'object' && value && !Array.isArray(value)
        ? (value as Record<string, unknown>)
        : {};
    return (
      <FormField
        label={label}
        required={required}
        {...(hint ? { hint } : {})}
      >
        <div className="rounded-md border border-bd-0 bg-bg-2 p-3">
          <SchemaForm
            schema={objectSchema}
            values={objectValue}
            onChange={onChange}
          />
        </div>
      </FormField>
    );
  }
  const enumValues = Array.isArray(definition.enum)
    ? definition.enum.map(String)
    : [];
  if (enumValues.length > 0) {
    return (
      <FormField
        label={label}
        required={required}
        {...(hint ? { hint } : {})}
      >
        <FormSelect
          value={value == null ? '' : String(value)}
          onChange={onChange}
          options={enumValues}
          className="w-full"
        />
      </FormField>
    );
  }
  return (
    <FormField
      label={label}
      required={required}
      {...(hint ? { hint } : {})}
    >
      <FormInput
        type={
          definition.type === 'integer' || definition.type === 'number'
            ? 'number'
            : 'text'
        }
        value={value == null ? '' : String(value)}
        onChange={(event) => {
          if (definition.type === 'integer' || definition.type === 'number') {
            onChange(event.target.value === '' ? undefined : Number(event.target.value));
          } else {
            onChange(event.target.value);
          }
        }}
      />
    </FormField>
  );
}

function TestResult({ result }: { result: intelligenceApi.ToolTestResult }) {
  const { t } = useTranslation('intelligence');
  return (
    <FormSection title={t('settings.tools.test.result')}>
      <div
        className={cn(
          'rounded-md border p-4',
          result.success
            ? 'border-green/25 bg-green/5'
            : 'border-red/25 bg-red/5',
        )}
      >
        <div className="flex items-center gap-2">
          {result.success ? (
            <CheckCircle2 className="h-4 w-4 text-green-soft" />
          ) : (
            <XCircle className="h-4 w-4 text-red-soft" />
          )}
          <span className="text-sm font-strong text-tx-0">
            {t(
              result.success
                ? 'settings.tools.test.success'
                : 'settings.tools.test.failed',
            )}
          </span>
          {result.duration_ms != null && (
            <Badge variant="outline" className="ml-auto font-mono">
              {result.duration_ms}ms
            </Badge>
          )}
        </div>
        {result.message && <p className="mt-2 text-xs leading-5 text-tx-2">{result.message}</p>}
      </div>
      <Tabs defaultValue="response">
        <TabsList>
          <TabsTrigger value="response">{t('settings.tools.test.response')}</TabsTrigger>
          <TabsTrigger value="request">{t('settings.tools.test.request')}</TabsTrigger>
        </TabsList>
        <TabsContent value="response" className="mt-3">
          <JsonBlock value={result.response ?? result} />
        </TabsContent>
        <TabsContent value="request" className="mt-3">
          <JsonBlock value={result.request ?? {}} />
        </TabsContent>
      </Tabs>
    </FormSection>
  );
}

function DependenciesDrawer({
  tool,
  onClose,
}: {
  tool: intelligenceApi.RegisteredTool | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('intelligence');
  const query = useQuery({
    queryKey: ['intelligence', 'tool-dependencies', tool?.id],
    queryFn: () => intelligenceApi.getToolDependencies(tool?.id ?? ''),
    enabled: Boolean(tool),
    retry: false,
  });
  if (!tool) return null;
  return (
    <FormDrawer
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={t('settings.tools.dependencies.title', { name: tool.name })}
      subtitle={t('settings.tools.dependencies.description')}
      width={620}
      footer={
        <Button variant="outline" onClick={onClose}>
          {t('common.close')}
        </Button>
      }
    >
      <DependencyContent dependencies={query.data} loading={query.isLoading} />
    </FormDrawer>
  );
}

function DependencyContent({
  dependencies,
  loading,
}: {
  dependencies: intelligenceApi.ToolDependencies | undefined;
  loading: boolean;
}) {
  const { t } = useTranslation('intelligence');
  if (loading) return <ToolsLoading compact />;
  if (!dependencies || dependencies.total === 0) {
    return (
      <div className="rounded-md border border-dashed border-bd-1 px-5 py-10 text-center">
        <CheckCircle2 className="mx-auto h-5 w-5 text-green-soft" />
        <div className="mt-2 text-sm font-strong text-tx-1">
          {t('settings.tools.dependencies.none')}
        </div>
      </div>
    );
  }
  const sections = [
    ['agent_profiles', dependencies.agent_profiles],
    ['automations', dependencies.automations],
    ['investigation_templates', dependencies.investigation_templates],
  ] as const;
  return (
    <div className="space-y-5">
      <div className="rounded-md border border-yellow/25 bg-yellow/5 p-3 text-xs leading-5 text-yellow-soft">
        {t('settings.tools.dependencies.warning', { count: dependencies.total })}
      </div>
      {sections.map(([key, items]) => (
        <FormSection
          key={key}
          title={`${t(`settings.tools.dependencies.${key}`)} · ${items.length}`}
        >
          {items.length > 0 ? (
            <div className="divide-y divide-bd-0 overflow-hidden rounded-md border border-bd-0">
              {items.map((item) => (
                <div key={item.id} className="flex items-center gap-3 bg-bg-2 px-3 py-2.5">
                  <Network className="h-3.5 w-3.5 text-tx-3" />
                  <span className="min-w-0 flex-1 truncate text-sm text-tx-1">
                    {item.name}
                  </span>
                  {'enabled' in item && (
                    <Badge variant="outline">
                      {t(
                        item.enabled
                          ? 'settings.tools.status.enabled'
                          : 'settings.tools.status.disabled',
                      )}
                    </Badge>
                  )}
                </div>
              ))}
            </div>
          ) : (
            <span className="text-xs text-tx-3">{t('settings.tools.dependencies.empty')}</span>
          )}
        </FormSection>
      ))}
    </div>
  );
}

function DisableConfirmationDrawer({
  target,
  pending,
  onClose,
  onConfirm,
}: {
  target: {
    tool: intelligenceApi.RegisteredTool;
    dependencies: intelligenceApi.ToolDependencies;
  } | null;
  pending: boolean;
  onClose: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation('intelligence');
  if (!target) return null;
  return (
    <FormDrawer
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={t('settings.tools.disable.title', { name: target.tool.name })}
      subtitle={t('settings.tools.disable.description')}
      width={540}
      footer={
        <>
          <Button variant="outline" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button variant="destructive" onClick={onConfirm} disabled={pending}>
            {t('settings.tools.disable.confirm')}
          </Button>
        </>
      }
    >
      <div className="rounded-md border border-yellow/25 bg-yellow/5 p-4">
        <div className="flex items-start gap-3">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-yellow-soft" />
          <p className="text-sm leading-6 text-tx-1">
            {t('settings.tools.disable.dependency_warning', {
              count: target.dependencies.total,
            })}
          </p>
        </div>
      </div>
      <div className="mt-5">
        <DependencyContent dependencies={target.dependencies} loading={false} />
      </div>
    </FormDrawer>
  );
}

function PolicyDefaultsDrawer({
  open,
  onClose,
  onSaved,
}: {
  open: boolean;
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const { t } = useTranslation('intelligence');
  const query = useQuery({
    queryKey: ['intelligence', 'tool-policy-defaults'],
    queryFn: intelligenceApi.getToolPolicyDefaults,
    enabled: open,
    retry: false,
  });
  const [riskModes, setRiskModes] = React.useState<
    Record<intelligenceApi.RiskLevel, intelligenceApi.ToolExecutionMode>
  >({
    l0: 'automatic',
    l1: 'confirmation',
    l2: 'single_approval',
    l3: 'dual_approval',
    l4: 'disabled',
  });
  const [environmentOverrides, setEnvironmentOverrides] = React.useState<
    Record<string, Partial<Record<intelligenceApi.RiskLevel, intelligenceApi.ToolExecutionMode>>>
  >({});
  React.useEffect(() => {
    if (!query.data) return;
    setRiskModes(query.data.risk_modes);
    setEnvironmentOverrides(query.data.environment_overrides ?? {});
  }, [query.data]);
  const mutation = useMutation({
    mutationFn: () =>
      intelligenceApi.updateToolPolicyDefaults({
        risk_modes: riskModes,
        environment_overrides: environmentOverrides,
      }),
    onSuccess: async () => {
      toast.success(t('settings.tools.feedback.defaults_saved'));
      await Promise.all([
        onSaved(),
        query.refetch(),
      ]);
      onClose();
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title={t('settings.tools.defaults.title')}
      subtitle={t('settings.tools.defaults.description')}
      width={720}
      footer={
        <>
          <Button variant="outline" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button onClick={() => mutation.mutate()} disabled={mutation.isPending}>
            {mutation.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </>
      }
    >
      {query.isLoading ? (
        <ToolsLoading compact />
      ) : (
        <>
          <FormSection
            title={t('settings.tools.defaults.risk_title')}
            description={t('settings.tools.defaults.risk_description')}
          >
            <div className="divide-y divide-bd-0 overflow-hidden rounded-md border border-bd-0">
              {RISK_ORDER.map((risk) => (
                <div
                  key={risk}
                  className="grid gap-3 bg-bg-2 px-3 py-3 sm:grid-cols-[190px_minmax(0,1fr)] sm:items-center"
                >
                  <div className="flex items-center gap-2">
                    <RiskBadge risk={risk} />
                    <span className="text-xs text-tx-2">
                      {t(`settings.tools.risk.${risk}.title`)}
                    </span>
                  </div>
                  <FormSelect
                    value={riskModes[risk]}
                    onChange={(value) =>
                      setRiskModes((current) => ({
                        ...current,
                        [risk]: value as intelligenceApi.ToolExecutionMode,
                      }))
                    }
                    options={allowedModes(risk).map((value) => ({
                      value,
                      label: t(`settings.tools.execution.${value}`),
                    }))}
                    className="w-full"
                  />
                </div>
              ))}
            </div>
          </FormSection>
          <FormSection
            title={t('settings.tools.defaults.environments_title')}
            description={t('settings.tools.defaults.environments_description')}
          >
            {(['development', 'staging', 'production'] as const).map((environment) => (
              <EnvironmentPolicyEditor
                key={environment}
                environment={environment}
                values={environmentOverrides[environment] ?? {}}
                onChange={(values) =>
                  setEnvironmentOverrides((current) => ({
                    ...current,
                    [environment]: values,
                  }))
                }
              />
            ))}
          </FormSection>
        </>
      )}
    </FormDrawer>
  );
}

function EnvironmentPolicyEditor({
  environment,
  values,
  onChange,
}: {
  environment: 'development' | 'staging' | 'production';
  values: Partial<Record<intelligenceApi.RiskLevel, intelligenceApi.ToolExecutionMode>>;
  onChange: (
    value: Partial<Record<intelligenceApi.RiskLevel, intelligenceApi.ToolExecutionMode>>,
  ) => void;
}) {
  const { t } = useTranslation('intelligence');
  const [expanded, setExpanded] = React.useState(environment === 'production');
  return (
    <div className="overflow-hidden rounded-md border border-bd-0">
      <button
        type="button"
        className="flex min-h-10 w-full items-center gap-2 bg-bg-2 px-3 text-left"
        onClick={() => setExpanded((value) => !value)}
      >
        {expanded ? (
          <ChevronDown className="h-3.5 w-3.5 text-tx-3" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5 text-tx-3" />
        )}
        <span className="text-sm font-strong text-tx-1">
          {t(`settings.tools.environments.${environment}`)}
        </span>
        <Badge variant="secondary" className="ml-auto">
          {Object.keys(values).length}
        </Badge>
      </button>
      {expanded && (
        <div className="grid gap-3 border-t border-bd-0 bg-bg-1 p-3 sm:grid-cols-2">
          {RISK_ORDER.map((risk) => (
            <FormField
              key={risk}
              label={`${risk.toUpperCase()} · ${t(`settings.tools.risk.${risk}.title`)}`}
            >
              <FormSelect
                value={values[risk] ?? ''}
                onChange={(value) => {
                  const next = { ...values };
                  if (value) next[risk] = value as intelligenceApi.ToolExecutionMode;
                  else delete next[risk];
                  onChange(next);
                }}
                options={[
                  { value: '', label: t('settings.tools.policy.inherit') },
                  ...allowedModes(risk).map((value) => ({
                    value,
                    label: t(`settings.tools.execution.${value}`),
                  })),
                ]}
                className="w-full"
              />
            </FormField>
          ))}
        </div>
      )}
    </div>
  );
}

function McpServersDrawer({
  open,
  onClose,
  onChanged,
}: {
  open: boolean;
  onClose: () => void;
  onChanged: () => Promise<void>;
}) {
  const { t } = useTranslation('intelligence');
  const queryClient = useQueryClient();
  const [editor, setEditor] = React.useState<intelligenceApi.McpServer | 'new' | null>(null);
  const [discoveries, setDiscoveries] = React.useState<
    Record<string, intelligenceApi.DiscoveredMcpTool[]>
  >({});
  const [selected, setSelected] = React.useState<Record<string, string[]>>({});
  const [removalTarget, setRemovalTarget] =
    React.useState<McpRemovalTarget | null>(null);
  const servers = useQuery({
    queryKey: ['intelligence', 'mcp-servers'],
    queryFn: intelligenceApi.listMcpServers,
    enabled: open,
    retry: false,
  });
  const testMutation = useMutation({
    mutationFn: intelligenceApi.testMcpServer,
    onSuccess: async (result) => {
      if (result.success) {
        setDiscoveries((current) => ({
          ...current,
          [result.server.id]: result.discovered_tools,
        }));
        setSelected((current) => ({
          ...current,
          [result.server.id]: result.discovered_tools.map((tool) => tool.name),
        }));
        toast.success(
          t('settings.tools.mcp.feedback.test_success', {
            count: result.discovered_tools.length,
          }),
        );
      } else {
        toast.error(result.error ?? t('settings.tools.mcp.feedback.test_failed'));
      }
      await servers.refetch();
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  const syncMutation = useMutation({
    mutationFn: ({ id, tools }: { id: string; tools: string[] }) =>
      intelligenceApi.syncMcpServer(id, tools),
    onSuccess: async (result) => {
      toast.success(
        t('settings.tools.mcp.feedback.sync_success', {
          count: result.tools.length,
        }),
      );
      await Promise.all([
        servers.refetch(),
        onChanged(),
      ]);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  const deleteMutation = useMutation({
    mutationFn: intelligenceApi.deleteMcpServer,
    onSuccess: async () => {
      setRemovalTarget(null);
      toast.success(t('settings.tools.mcp.feedback.deleted'));
      await Promise.all([servers.refetch(), onChanged()]);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  const inspectRemovalMutation = useMutation({
    mutationFn: async (server: intelligenceApi.McpServer) => {
      const detail = await intelligenceApi.getMcpServer(server.id);
      const dependencies = await Promise.all(
        detail.tools.map((tool) =>
          intelligenceApi.getToolDependencies(tool.id || tool.name),
        ),
      );
      return { server, tools: detail.tools, dependencies };
    },
    onSuccess: setRemovalTarget,
    onError: (error) => toast.error(errorMessage(error)),
  });
  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ['intelligence', 'mcp-servers'] }),
      onChanged(),
    ]);
  };
  return (
    <>
      <FormDrawer
        open={open}
        onOpenChange={(next) => {
          if (!next) onClose();
        }}
        title={t('settings.tools.mcp.title')}
        subtitle={t('settings.tools.mcp.description')}
        width={760}
        footer={
          <Button onClick={() => setEditor('new')}>
            <Plus className="h-3.5 w-3.5" />
            {t('settings.tools.actions.add_mcp')}
          </Button>
        }
      >
        {servers.isLoading ? (
          <ToolsLoading compact />
        ) : servers.isError ? (
          <ToolsError compact onRetry={() => servers.refetch()} />
        ) : servers.data?.length ? (
          <div className="space-y-3">
            {servers.data.map((server) => {
              const discovered = discoveries[server.id] ?? [];
              const selectedNames = selected[server.id] ?? [];
              return (
                <section
                  key={server.id}
                  className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1"
                >
                  <div className="flex flex-wrap items-center gap-3 px-4 py-3">
                    <span className="grid h-9 w-9 place-items-center rounded-md border border-bd-0 bg-bg-2">
                      <Server className="h-4 w-4 text-tx-2" />
                    </span>
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="font-strong text-tx-0">{server.name}</span>
                        <McpStatusBadge status={server.status} />
                      </div>
                      <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-xs text-tx-3">
                        <span>{t(`settings.tools.mcp.transport.${server.transport}`)}</span>
                        <span>
                          {t('settings.tools.mcp.tool_count', {
                            count: server.tool_count ?? 0,
                          })}
                        </span>
                        {server.last_error && (
                          <span className="text-red-soft">{server.last_error}</span>
                        )}
                      </div>
                    </div>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => testMutation.mutate(server.id)}
                      disabled={testMutation.isPending}
                    >
                      <FlaskConical className="h-3.5 w-3.5" />
                      {t('settings.tools.mcp.test_connection')}
                    </Button>
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button variant="ghost" size="icon" aria-label={t('common.actions')}>
                          <Ellipsis className="h-4 w-4" />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        <DropdownMenuItem onSelect={() => setEditor(server)}>
                          <Pencil className="h-3.5 w-3.5" />
                          {t('common.edit')}
                        </DropdownMenuItem>
                        <DropdownMenuItem
                          className="text-red-soft"
                          disabled={inspectRemovalMutation.isPending}
                          onSelect={() => inspectRemovalMutation.mutate(server)}
                        >
                          <Unplug className="h-3.5 w-3.5" />
                          {t('settings.tools.mcp.remove')}
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                  {discovered.length > 0 && (
                    <div className="border-t border-bd-0 bg-bg-2 p-4">
                      <div className="flex items-center gap-2">
                        <span className="text-xs font-strong text-tx-1">
                          {t('settings.tools.mcp.discovered', {
                            count: discovered.length,
                          })}
                        </span>
                        <span className="text-xs text-tx-3">
                          {t('settings.tools.mcp.new_tools_disabled')}
                        </span>
                        <Button
                          size="sm"
                          className="ml-auto"
                          onClick={() =>
                            syncMutation.mutate({
                              id: server.id,
                              tools: selectedNames,
                            })
                          }
                          disabled={selectedNames.length === 0 || syncMutation.isPending}
                        >
                          <RefreshCw className="h-3.5 w-3.5" />
                          {t('settings.tools.mcp.sync_selected')}
                        </Button>
                      </div>
                      <div className="mt-3 max-h-60 divide-y divide-bd-0 overflow-auto rounded-md border border-bd-0 bg-bg-1">
                        {discovered.map((tool) => (
                          <label
                            key={tool.name}
                            className="flex cursor-pointer items-start gap-3 px-3 py-2.5"
                          >
                            <Checkbox
                              checked={selectedNames.includes(tool.name)}
                              onCheckedChange={() =>
                                setSelected((current) => ({
                                  ...current,
                                  [server.id]: toggleListValue(
                                    current[server.id] ?? [],
                                    tool.name,
                                  ),
                                }))
                              }
                            />
                            <span className="min-w-0">
                              <span className="block font-mono text-xs font-strong text-tx-0">
                                {tool.name}
                              </span>
                              <span className="mt-0.5 block text-xs text-tx-3">
                                {tool.description}
                              </span>
                            </span>
                          </label>
                        ))}
                      </div>
                    </div>
                  )}
                </section>
              );
            })}
          </div>
        ) : (
          <div className="rounded-lg border border-dashed border-bd-1 px-6 py-12 text-center">
            <Server className="mx-auto h-6 w-6 text-tx-3" />
            <div className="mt-3 text-sm font-strong text-tx-1">
              {t('settings.tools.mcp.empty_title')}
            </div>
            <p className="mt-1 text-xs text-tx-3">
              {t('settings.tools.mcp.empty_description')}
            </p>
            <Button className="mt-4" onClick={() => setEditor('new')}>
              <Plus className="h-3.5 w-3.5" />
              {t('settings.tools.actions.add_mcp')}
            </Button>
          </div>
        )}
      </FormDrawer>
      <McpServerEditorDrawer
        target={editor}
        onClose={() => setEditor(null)}
        onSaved={refresh}
      />
      <McpRemovalDrawer
        target={removalTarget}
        pending={deleteMutation.isPending}
        onClose={() => setRemovalTarget(null)}
        onConfirm={(server) => deleteMutation.mutate(server.id)}
      />
    </>
  );
}

function McpRemovalDrawer({
  target,
  pending,
  onClose,
  onConfirm,
}: {
  target: McpRemovalTarget | null;
  pending: boolean;
  onClose: () => void;
  onConfirm: (server: intelligenceApi.McpServer) => void;
}) {
  const { t } = useTranslation('intelligence');
  if (!target) return null;
  const enabledTools = target.tools.filter((tool) => tool.enabled);
  const dependencyCount = target.dependencies.reduce(
    (total, dependency) => total + dependency.total,
    0,
  );
  const canRemove = enabledTools.length === 0 && dependencyCount === 0;
  return (
    <FormDrawer
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={t('settings.tools.mcp.remove_drawer.title', {
        name: target.server.name,
      })}
      subtitle={t('settings.tools.mcp.remove_drawer.description')}
      width={620}
      footer={
        <>
          <Button variant="outline" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button
            variant="destructive"
            disabled={!canRemove || pending}
            onClick={() => onConfirm(target.server)}
          >
            {t('settings.tools.mcp.remove_drawer.confirm')}
          </Button>
        </>
      }
    >
      <div
        className={cn(
          'rounded-md border p-3 text-xs leading-5',
          canRemove
            ? 'border-green/20 bg-green/5 text-green-soft'
            : 'border-yellow/25 bg-yellow/5 text-yellow-soft',
        )}
      >
        {canRemove
          ? t('settings.tools.mcp.remove_drawer.safe')
          : t('settings.tools.mcp.remove_drawer.blocked', {
              enabled: enabledTools.length,
              dependencies: dependencyCount,
            })}
      </div>
      <FormSection
        title={t('settings.tools.mcp.remove_drawer.tools_title')}
        className="mt-6"
      >
        {target.tools.length === 0 ? (
          <p className="text-xs text-tx-3">
            {t('settings.tools.mcp.remove_drawer.no_tools')}
          </p>
        ) : (
          <div className="divide-y divide-bd-0 overflow-hidden rounded-md border border-bd-0">
            {target.tools.map((tool, index) => {
              const dependencies = target.dependencies[index];
              return (
                <div key={tool.id} className="flex items-center gap-3 bg-bg-2 px-3 py-2.5">
                  <span className="min-w-0 flex-1 truncate font-mono text-xs text-tx-1">
                    {tool.name}
                  </span>
                  {tool.enabled && (
                    <Badge variant="outline" className="border-yellow/30 text-yellow-soft">
                      {t('settings.tools.status.enabled')}
                    </Badge>
                  )}
                  <Badge variant="secondary">
                    {t('settings.tools.mcp.remove_drawer.dependency_count', {
                      count: dependencies?.total ?? 0,
                    })}
                  </Badge>
                </div>
              );
            })}
          </div>
        )}
      </FormSection>
    </FormDrawer>
  );
}

function McpServerEditorDrawer({
  target,
  onClose,
  onSaved,
}: {
  target: intelligenceApi.McpServer | 'new' | null;
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const { t } = useTranslation('intelligence');
  const existing = target && target !== 'new' ? target : null;
  const [name, setName] = React.useState('');
  const [transport, setTransport] =
    React.useState<intelligenceApi.McpTransport>('streamable_http');
  const [endpointUrl, setEndpointUrl] = React.useState('');
  const [authType, setAuthType] =
    React.useState<intelligenceApi.McpAuthType>('none');
  const [authHeader, setAuthHeader] = React.useState('');
  const [credential, setCredential] = React.useState('');
  const [privateOnly, setPrivateOnly] = React.useState(true);
  const [allowedDomains, setAllowedDomains] = React.useState('');
  const [allowedCidrs, setAllowedCidrs] = React.useState('');
  const [tlsVerify, setTlsVerify] = React.useState(true);
  const [timeoutMs, setTimeoutMs] = React.useState('10000');
  const [maxResponseBytes, setMaxResponseBytes] = React.useState('1048576');
  const [enabled, setEnabled] = React.useState(true);
  React.useEffect(() => {
    setName(existing?.name ?? '');
    setTransport(existing?.transport ?? 'streamable_http');
    setEndpointUrl(existing?.endpoint_url ?? '');
    setAuthType(existing?.auth_type ?? 'none');
    setAuthHeader(existing?.auth_header ?? '');
    setCredential('');
    setPrivateOnly(existing?.private_only ?? true);
    setAllowedDomains((existing?.allowed_domains ?? []).join('\n'));
    setAllowedCidrs((existing?.allowed_cidrs ?? []).join('\n'));
    setTlsVerify(existing?.tls_verify ?? true);
    setTimeoutMs(String(existing?.timeout_ms ?? 10000));
    setMaxResponseBytes(String(existing?.max_response_bytes ?? 1048576));
    setEnabled(existing?.enabled ?? true);
  }, [existing, target]);
  const mutation = useMutation({
    mutationFn: () => {
      const input: intelligenceApi.McpServerInput = {
        name,
        transport,
        auth_type: authType,
        private_only: privateOnly,
        allowed_domains: splitLines(allowedDomains),
        allowed_cidrs: splitLines(allowedCidrs),
        follow_redirects: false,
        tls_verify: tlsVerify,
        timeout_ms: Number(timeoutMs),
        max_response_bytes: Number(maxResponseBytes),
        enabled,
        ...(endpointUrl ? { endpoint_url: endpointUrl } : {}),
        ...(authHeader ? { auth_header: authHeader } : {}),
        ...(credential ? { credential } : {}),
      };
      return existing
        ? intelligenceApi.updateMcpServer(existing.id, input)
        : intelligenceApi.createMcpServer(input);
    },
    onSuccess: async () => {
      toast.success(
        t(
          existing
            ? 'settings.tools.mcp.feedback.updated'
            : 'settings.tools.mcp.feedback.created',
        ),
      );
      await onSaved();
      onClose();
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  if (!target) return null;
  return (
    <FormDrawer
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={t(
        existing
          ? 'settings.tools.mcp.editor.edit_title'
          : 'settings.tools.mcp.editor.create_title',
      )}
      subtitle={t('settings.tools.mcp.editor.description')}
      width={680}
      footer={
        <>
          <Button variant="outline" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button
            onClick={() => mutation.mutate()}
            disabled={mutation.isPending || !name.trim() || !endpointUrl.trim()}
          >
            {mutation.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </>
      }
    >
      <FormSection title={t('settings.tools.mcp.editor.basic')}>
        <FormField label={t('settings.tools.mcp.fields.name')} required>
          <FormInput
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder={t('settings.tools.mcp.fields.name_placeholder')}
          />
        </FormField>
        <FormField label={t('settings.tools.mcp.fields.transport')} required>
          <FormSelect
            value={transport}
            onChange={(value) => setTransport(value as intelligenceApi.McpTransport)}
            options={(
              ['streamable_http', 'sse', 'stdio', 'unix_socket'] as intelligenceApi.McpTransport[]
            ).map((value) => ({
              value,
              label: t(`settings.tools.mcp.transport.${value}`),
            }))}
            className="w-full"
          />
        </FormField>
        {transport === 'streamable_http' ? (
          <FormField label={t('settings.tools.mcp.fields.endpoint')} required>
            <FormInput
              type="url"
              value={endpointUrl}
              onChange={(event) => setEndpointUrl(event.target.value)}
              placeholder={t('settings.tools.mcp.fields.endpoint_placeholder')}
            />
          </FormField>
        ) : (
          <div className="rounded-md border border-yellow/25 bg-yellow/5 p-3 text-xs leading-5 text-yellow-soft">
            {t('settings.tools.mcp.editor.transport_restricted')}
          </div>
        )}
      </FormSection>
      <FormSection
        title={t('settings.tools.mcp.editor.authentication')}
        description={
          existing?.credential_set
            ? t('settings.tools.mcp.editor.credential_set', {
                last4: existing.credential_last4 ?? '',
              })
            : t('settings.tools.mcp.editor.credential_not_set')
        }
      >
        <FormField label={t('settings.tools.mcp.fields.auth_type')}>
          <FormSelect
            value={authType}
            onChange={(value) => setAuthType(value as intelligenceApi.McpAuthType)}
            options={(
              [
                'none',
                'bearer_token',
                'api_key',
                'oauth',
                'mtls',
                'internal_service_identity',
              ] as intelligenceApi.McpAuthType[]
            ).map((value) => ({
              value,
              label: t(`settings.tools.mcp.auth.${value}`),
            }))}
            className="w-full"
          />
        </FormField>
        {['api_key', 'internal_service_identity'].includes(authType) && (
          <FormField label={t('settings.tools.mcp.fields.auth_header')}>
            <FormInput
              value={authHeader}
              onChange={(event) => setAuthHeader(event.target.value)}
              placeholder={t('settings.tools.mcp.fields.auth_header_placeholder')}
            />
          </FormField>
        )}
        {authType !== 'none' && (
          <FormField
            label={t('settings.tools.mcp.fields.credential')}
            hint={t('settings.tools.mcp.fields.credential_hint')}
            required={!existing?.credential_set}
          >
            <FormInput
              type="password"
              autoComplete="new-password"
              value={credential}
              onChange={(event) => setCredential(event.target.value)}
              placeholder={t('settings.tools.mcp.fields.credential_placeholder')}
            />
          </FormField>
        )}
      </FormSection>
      <FormSection title={t('settings.tools.mcp.editor.network')}>
        <ToggleField
          label={t('settings.tools.mcp.fields.private_only')}
          hint={t('settings.tools.mcp.fields.private_only_hint')}
          checked={privateOnly}
          onChange={setPrivateOnly}
        />
        <FormRow>
          <FormField
            label={t('settings.tools.mcp.fields.allowed_domains')}
            hint={t('settings.tools.mcp.fields.one_per_line')}
          >
            <textarea
              className="min-h-24 rounded-md border border-bd-1 bg-bg-2 px-3 py-2 font-mono text-xs text-tx-0"
              value={allowedDomains}
              onChange={(event) => setAllowedDomains(event.target.value)}
            />
          </FormField>
          <FormField
            label={t('settings.tools.mcp.fields.allowed_cidrs')}
            hint={t('settings.tools.mcp.fields.one_per_line')}
          >
            <textarea
              className="min-h-24 rounded-md border border-bd-1 bg-bg-2 px-3 py-2 font-mono text-xs text-tx-0"
              value={allowedCidrs}
              onChange={(event) => setAllowedCidrs(event.target.value)}
            />
          </FormField>
        </FormRow>
        <ToggleField
          label={t('settings.tools.mcp.fields.tls_verify')}
          checked={tlsVerify}
          onChange={setTlsVerify}
        />
        <div className="rounded-md border border-green/20 bg-green/5 p-3 text-xs text-green-soft">
          {t('settings.tools.mcp.fields.redirects_blocked')}
        </div>
      </FormSection>
      <FormSection title={t('settings.tools.mcp.editor.runtime')}>
        <FormRow>
          <FormField label={t('settings.tools.mcp.fields.timeout_ms')}>
            <FormInput
              type="number"
              min={1000}
              max={120000}
              value={timeoutMs}
              onChange={(event) => setTimeoutMs(event.target.value)}
            />
          </FormField>
          <FormField label={t('settings.tools.mcp.fields.max_response_bytes')}>
            <FormInput
              type="number"
              min={1024}
              max={16777216}
              value={maxResponseBytes}
              onChange={(event) => setMaxResponseBytes(event.target.value)}
            />
          </FormField>
        </FormRow>
        <ToggleField
          label={t('settings.tools.mcp.fields.enabled')}
          checked={enabled}
          onChange={setEnabled}
        />
      </FormSection>
    </FormDrawer>
  );
}

function McpStatusBadge({ status }: { status: intelligenceApi.McpServer['status'] }) {
  const { t } = useTranslation('intelligence');
  return (
    <Badge
      variant="outline"
      className={cn(
        'text-type-micro',
        status === 'healthy' && 'border-green/30 text-green-soft',
        status === 'error' && 'border-red/30 text-red-soft',
        status === 'connecting' && 'border-yellow/30 text-yellow-soft',
      )}
    >
      {t(`settings.tools.mcp.status.${status}`, { defaultValue: status })}
    </Badge>
  );
}

function ToggleField({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className="flex items-center justify-between gap-4 rounded-md border border-bd-0 bg-bg-2 px-3 py-2.5">
      <span>
        <span className="block text-xs font-strong text-tx-1">{label}</span>
        {hint && <span className="mt-0.5 block text-xs text-tx-3">{hint}</span>}
      </span>
      <Switch checked={checked} onCheckedChange={onChange} />
    </label>
  );
}

function CallRecords({
  calls,
  loading,
}: {
  calls: intelligenceApi.ToolCallRecord[];
  loading: boolean;
}) {
  const { t } = useTranslation('intelligence');
  if (loading) return <ToolsLoading compact />;
  if (calls.length === 0) {
    return (
      <div className="rounded-md border border-dashed border-bd-1 px-5 py-10 text-center text-xs text-tx-3">
        {t('settings.tools.calls.empty')}
      </div>
    );
  }
  return (
    <div className="space-y-2">
      {calls.map((call) => (
        <details key={call.id} className="overflow-hidden rounded-md border border-bd-0 bg-bg-1">
          <summary className="flex cursor-pointer list-none items-center gap-3 px-3 py-2.5">
            {call.status === 'success' ? (
              <CheckCircle2 className="h-3.5 w-3.5 text-green-soft" />
            ) : (
              <XCircle className="h-3.5 w-3.5 text-red-soft" />
            )}
            <span className="min-w-0 flex-1 truncate text-xs text-tx-1">
              {t(`settings.tools.calls.source.${call.call_source}`, {
                defaultValue: call.call_source,
              })}
            </span>
            <RiskBadge risk={call.risk} />
            <span className="font-mono text-xs tabular-nums text-tx-3">
              {call.duration_ms}ms
            </span>
            <span className="font-mono text-type-micro text-tx-3">
              {formatTimestamp(call.created_at)}
            </span>
          </summary>
          <div className="space-y-3 border-t border-bd-0 bg-bg-2 p-3">
            <div>
              <div className="mb-1 text-type-micro font-strong uppercase text-tx-3">
                {t('settings.tools.calls.arguments')}
              </div>
              <JsonBlock value={call.input} />
            </div>
            <div>
              <div className="mb-1 text-type-micro font-strong uppercase text-tx-3">
                {t('settings.tools.calls.policy_decision')}
              </div>
              <JsonBlock value={call.policy_decision} />
            </div>
            {(call.error || call.output_summary) && (
              <p className={cn('text-xs text-tx-2', call.error && 'text-red-soft')}>
                {call.error ?? call.output_summary}
              </p>
            )}
          </div>
        </details>
      ))}
    </div>
  );
}

function DetailGrid({ rows }: { rows: Array<[string, React.ReactNode]> }) {
  return (
    <dl className="divide-y divide-bd-0 overflow-hidden rounded-md border border-bd-0">
      {rows.map(([label, value]) => (
        <div key={label} className="grid grid-cols-[150px_minmax(0,1fr)] gap-4 bg-bg-2 px-3 py-2.5">
          <dt className="text-xs text-tx-3">{label}</dt>
          <dd className="min-w-0 text-sm text-tx-1">{value}</dd>
        </div>
      ))}
    </dl>
  );
}

function MiniStat({
  label,
  value,
  warning = false,
}: {
  label: string;
  value: string;
  warning?: boolean;
}) {
  return (
    <div className="rounded-md border border-bd-0 bg-bg-2 p-3">
      <div className="text-type-micro uppercase tracking-[0.04em] text-tx-3">{label}</div>
      <div
        className={cn(
          'mt-1 font-mono text-sm font-strong tabular-nums text-tx-0',
          warning && 'text-yellow-soft',
        )}
      >
        {value}
      </div>
    </div>
  );
}

function JsonBlock({ value }: { value: unknown }) {
  return (
    <pre className="max-h-72 overflow-auto rounded-md border border-bd-0 bg-bg-1 p-3 font-mono text-type-micro leading-5 text-tx-2">
      {JSON.stringify(value, null, 2)}
    </pre>
  );
}

function ToolsLoading({ compact = false }: { compact?: boolean }) {
  const { t } = useTranslation('intelligence');
  return (
    <div
      className={cn(
        'flex items-center justify-center rounded-lg border border-bd-0 bg-bg-1',
        compact ? 'min-h-32' : 'min-h-80',
      )}
    >
      <RefreshCw className="h-4 w-4 animate-spin text-tx-3" />
      <span className="ml-2 text-xs text-tx-3">{t('settings.tools.loading')}</span>
    </div>
  );
}

function ToolsError({
  onRetry,
  compact = false,
}: {
  onRetry: () => void;
  compact?: boolean;
}) {
  const { t } = useTranslation('intelligence');
  return (
    <div
      className={cn(
        'flex flex-col items-center justify-center rounded-lg border border-red/20 bg-bg-1 px-6 text-center',
        compact ? 'min-h-40' : 'min-h-80',
      )}
    >
      <XCircle className="h-5 w-5 text-red-soft" />
      <div className="mt-3 text-sm font-strong text-tx-1">
        {t('settings.tools.load_error')}
      </div>
      <Button className="mt-4" size="sm" variant="outline" onClick={onRetry}>
        <RefreshCw className="h-3.5 w-3.5" />
        {t('settings.tools.retry')}
      </Button>
    </div>
  );
}

function ToolsEmpty({ onAddMcp }: { onAddMcp: () => void }) {
  const { t } = useTranslation('intelligence');
  return (
    <div className="mt-4 flex min-h-72 flex-col items-center justify-center rounded-lg border border-dashed border-bd-1 bg-bg-1 px-6 text-center">
      <Wrench className="h-6 w-6 text-tx-3" />
      <div className="mt-3 text-sm font-strong text-tx-1">
        {t('settings.tools.empty_title')}
      </div>
      <p className="mt-1 max-w-md text-xs leading-5 text-tx-3">
        {t('settings.tools.empty_description')}
      </p>
      <Button className="mt-4" onClick={onAddMcp}>
        <Plus className="h-3.5 w-3.5" />
        {t('settings.tools.actions.add_mcp')}
      </Button>
    </div>
  );
}

function normalizeTool(
  raw: intelligenceApi.RegisteredTool,
): intelligenceApi.RegisteredTool {
  const risk = raw.risk ?? 'l0';
  const access = raw.access ?? 'read_only';
  const source = raw.source ?? { kind: 'builtin', label: 'builtin' };
  return {
    ...raw,
    id: raw.id ?? raw.name,
    display_name: raw.display_name ?? raw.name,
    description: raw.description ?? '',
    technical_description: raw.technical_description ?? raw.description ?? '',
    domain: raw.domain ?? inferDomain(raw.name),
    category: raw.category ?? inferCategory(raw.name),
    source,
    risk,
    execution_mode:
      raw.execution_mode ??
      (access === 'read_only' ? 'automatic' : 'confirmation'),
    enabled: raw.enabled ?? true,
    available_to_agent: raw.available_to_agent ?? raw.enabled ?? true,
    status: raw.status ?? (raw.enabled === false ? 'disabled' : 'healthy'),
    input_schema: raw.input_schema ?? {},
    output_schema: raw.output_schema ?? {},
    capabilities: raw.capabilities ?? {
      read_only: access === 'read_only',
      supports_dry_run: true,
      idempotent: access === 'read_only',
      streaming: false,
    },
    limits: raw.limits ?? {
      timeout_ms: 30000,
      max_calls_per_run: 32,
      max_response_bytes: 1048576,
    },
    environment_overrides: raw.environment_overrides ?? {},
    tags: raw.tags ?? [inferCategory(raw.name)],
    statistics: raw.statistics ?? { calls_24h: 0 },
    access,
  };
}

function inferDomain(name: string): intelligenceApi.ToolDomain {
  if (name.includes('alert') || name.includes('on_call') || name.includes('schedule')) {
    return 'alerts_on_call';
  }
  if (name.includes('report') || name.includes('dashboard')) return 'dashboard_reports';
  if (name.includes('operation') || name.includes('incident')) return 'automation';
  return 'observability';
}

function inferCategory(name: string): string {
  if (name.includes('log')) return 'Logs';
  if (name.includes('metric')) return 'Metrics';
  if (name.includes('trace')) return 'Trace';
  if (name.includes('rum')) return 'APM · User Experience';
  if (name.includes('profile')) return 'Profiles';
  if (name.includes('report')) return 'Reports';
  if (name.includes('alert')) return 'Alert';
  if (name.includes('on_call') || name.includes('schedule')) return 'On-call';
  return 'Operations';
}

function groupTools(
  tools: intelligenceApi.RegisteredTool[],
  mode: GroupMode,
): ToolGroupModel[] {
  if (mode === 'none') {
    return [
      {
        key: 'all',
        title: '',
        description: '',
        tools,
      },
    ];
  }
  const map = new Map<string, intelligenceApi.RegisteredTool[]>();
  for (const tool of tools) {
    const key =
      mode === 'domain'
        ? tool.domain
        : tool.source.kind === 'mcp'
          ? tool.source.server_name ?? tool.source.label
          : 'builtin';
    const group = map.get(key) ?? [];
    group.push(tool);
    map.set(key, group);
  }
  const keys =
    mode === 'domain'
      ? DOMAIN_ORDER.filter((key) => map.has(key))
      : [...map.keys()].sort((left, right) => left.localeCompare(right));
  return keys.map((key) => ({
    key: `${mode}:${key}`,
    title: key,
    description: '',
    tools: (map.get(key) ?? []).sort((left, right) => left.name.localeCompare(right.name)),
  }));
}

function useLocalizedGroups(groups: ToolGroupModel[]): ToolGroupModel[] {
  const { t } = useTranslation('intelligence');
  return groups.map((group) => {
    const rawKey = group.key.split(':').slice(1).join(':');
    if (group.key.startsWith('domain:')) {
      return {
        ...group,
        title: t(`settings.tools.domains.${rawKey}.title`),
        description: t(`settings.tools.domains.${rawKey}.description`),
      };
    }
    if (rawKey === 'builtin') {
      return {
        ...group,
        title: t('settings.tools.sources.builtin'),
        description: t('settings.tools.grouping.builtin_description'),
      };
    }
    if (group.key === 'all') {
      return {
        ...group,
        title: t('settings.tools.grouping.all_tools'),
      };
    }
    return {
      ...group,
      title: rawKey,
      description: t('settings.tools.grouping.mcp_description'),
    };
  });
}

function schemaProperties(
  schema: Record<string, unknown>,
): Array<[string, Record<string, unknown>]> {
  const properties =
    schema.properties && typeof schema.properties === 'object' && !Array.isArray(schema.properties)
      ? (schema.properties as Record<string, unknown>)
      : {};
  return Object.entries(properties).map(([name, definition]) => [
    name,
    definition && typeof definition === 'object' && !Array.isArray(definition)
      ? (definition as Record<string, unknown>)
      : {},
  ]);
}

function schemaDefaults(schema: Record<string, unknown>): Record<string, unknown> {
  return Object.fromEntries(
    schemaProperties(schema).map(([name, definition]) => {
      if ('default' in definition) return [name, definition.default];
      if (definition.type === 'object') return [name, schemaDefaults(definition)];
      if (definition.type === 'boolean') return [name, false];
      return [name, ''];
    }),
  );
}

function allowedModes(
  risk: intelligenceApi.RiskLevel,
): intelligenceApi.ToolExecutionMode[] {
  if (risk === 'l4') return ['dual_approval', 'disabled'];
  if (risk === 'l3' || risk === 'l2') {
    return ['single_approval', 'dual_approval', 'disabled'];
  }
  return EXECUTION_MODES;
}

function defaultMode(
  risk: intelligenceApi.RiskLevel,
): intelligenceApi.ToolExecutionMode {
  if (risk === 'l0') return 'automatic';
  if (risk === 'l1') return 'confirmation';
  if (risk === 'l2') return 'single_approval';
  if (risk === 'l3') return 'dual_approval';
  return 'disabled';
}

function toggleListValue(values: string[], value: string): string[] {
  return values.includes(value)
    ? values.filter((candidate) => candidate !== value)
    : [...values, value];
}

function splitLines(value: string): string[] {
  return value
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${Math.round(value / 1024)} KiB`;
  return `${(value / (1024 * 1024)).toFixed(1)} MiB`;
}

function formatTimestamp(micros: number): string {
  const milliseconds = micros > 10_000_000_000_000 ? Math.floor(micros / 1000) : micros;
  return new Intl.DateTimeFormat(undefined, {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(milliseconds));
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

function readCollapsedGroups(): Set<string> {
  if (typeof window === 'undefined') return new Set();
  try {
    const value = JSON.parse(window.localStorage.getItem(COLLAPSED_STORAGE_KEY) ?? '[]');
    return new Set(Array.isArray(value) ? value.map(String) : []);
  } catch {
    return new Set();
  }
}

function writeCollapsedGroups(groups: Set<string>) {
  if (typeof window === 'undefined') return;
  window.localStorage.setItem(COLLAPSED_STORAGE_KEY, JSON.stringify([...groups]));
}
