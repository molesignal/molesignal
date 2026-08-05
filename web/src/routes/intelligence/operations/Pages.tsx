import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  ArrowRight,
  Bot,
  Check,
  CircleDot,
  Clock3,
  FlaskConical,
  Pencil,
  Play,
  Plus,
  ShieldCheck,
  X,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useParams } from 'react-router-dom';

import * as intelligenceApi from '@/api/intelligence';
import { formatMicrosActive } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import { cn } from '@/shell/lib/cn';
import { Badge } from '@/shell/ui/badge';
import { Button } from '@/shell/ui/button';
import { toast } from '@/shell/ui/sonner';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shell/ui/table';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shell/ui/tabs';

import {
  AutomationEditorDrawer,
  InvestigationEditorDrawer,
  type AutomationEditorTarget,
  type InvestigationEditorTarget,
} from './Editors';

const EMPTY_TOOLS: intelligenceApi.RegisteredTool[] = [];

export function InvestigationsPage() {
  const { t } = useTranslation('intelligence');
  const [editor, setEditor] = React.useState<InvestigationEditorTarget>(null);
  const investigations = useQuery({
    queryKey: ['intelligence', 'investigations'],
    queryFn: intelligenceApi.listInvestigations,
    retry: false,
  });

  return (
    <ModulePage
      title={t('investigations.title')}
      description={t('investigations.description')}
      action={
        <Button size="sm" onClick={() => setEditor('new')}>
          <Plus /> {t('investigations.create')}
        </Button>
      }
    >
      {investigations.isLoading ? (
        <ProductState variant="loading" />
      ) : investigations.isError ? (
        <ProductState variant="error" error={investigations.error} />
      ) : investigations.data?.length ? (
        <div className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('investigations.columns.task')}</TableHead>
                <TableHead>{t('common.status')}</TableHead>
                <TableHead>{t('investigations.columns.current_step')}</TableHead>
                <TableHead>{t('common.updated')}</TableHead>
                <TableHead className="w-12" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {investigations.data.map((item) => (
                <TableRow key={item.id}>
                  <TableCell>
                    <Link
                      to={`/intelligence/investigations/${encodeURIComponent(item.id)}`}
                      className="font-strong text-tx-0 hover:text-indigo"
                    >
                      {item.title}
                    </Link>
                    {item.summary && <p className="mt-0.5 max-w-xl truncate text-xs text-tx-3">{item.summary}</p>}
                  </TableCell>
                  <TableCell><StateBadge value={item.status} /></TableCell>
                  <TableCell className="max-w-xs truncate text-tx-2">{item.current_step ?? '—'}</TableCell>
                  <TableCell className="whitespace-nowrap text-xs text-tx-3">
                    {formatMicrosActive(item.updated_at)}
                  </TableCell>
                  <TableCell>
                    <div className="flex items-center justify-end">
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label={t('investigations.edit')}
                        onClick={() => setEditor(item)}
                      >
                        <Pencil />
                      </Button>
                      <Button variant="ghost" size="icon" asChild aria-label={t('common.open')}>
                        <Link to={`/intelligence/investigations/${encodeURIComponent(item.id)}`}>
                          <ArrowRight />
                        </Link>
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      ) : (
        <ProductState
          variant="empty"
          title={t('investigations.empty_title')}
          description={t('investigations.empty_description')}
          action={<Button onClick={() => setEditor('new')}>{t('investigations.create')}</Button>}
        />
      )}
      <InvestigationEditorDrawer
        target={editor}
        onClose={() => setEditor(null)}
      />
    </ModulePage>
  );
}

export function InvestigationDetailPage() {
  const { t } = useTranslation('intelligence');
  const { id = '' } = useParams();
  const [editor, setEditor] = React.useState<InvestigationEditorTarget>(null);
  const detail = useQuery({
    queryKey: ['intelligence', 'investigation', id],
    queryFn: () => intelligenceApi.getInvestigation(id),
    enabled: Boolean(id),
    retry: false,
  });
  if (detail.isLoading) return <PageState><ProductState variant="loading" /></PageState>;
  if (detail.isError) return <PageState><ProductState variant="error" error={detail.error} /></PageState>;
  if (!detail.data) return null;
  const { investigation, steps, evidence, hypotheses } = detail.data;
  return (
    <ModulePage
      title={investigation.title}
      description={t('investigation_detail.description')}
      action={
        <div className="flex items-center gap-2">
          <StateBadge value={investigation.status} />
          <Button
            size="sm"
            variant="outline"
            onClick={() => setEditor(investigation)}
          >
            <Pencil /> {t('common.edit')}
          </Button>
        </div>
      }
      backTo="/intelligence/investigations"
    >
      <div className="mb-4 flex flex-wrap items-center gap-2 text-xs text-tx-3">
        {investigation.confidence && (
          <Badge variant="outline">
            {t('investigation_detail.confidence')}: {t(`confidence.${investigation.confidence}`)}
          </Badge>
        )}
        {investigation.current_step && <Badge variant="secondary">{investigation.current_step}</Badge>}
        <span>{formatMicrosActive(investigation.updated_at)}</span>
      </div>
      <Tabs defaultValue="overview" className="min-h-0">
        <TabsList className="max-w-full justify-start overflow-x-auto">
          {['overview', 'timeline', 'evidence', 'queries', 'operations', 'runs'].map((tab) => (
            <TabsTrigger key={tab} value={tab}>{t(`investigation_detail.tabs.${tab}`)}</TabsTrigger>
          ))}
        </TabsList>
        <TabsContent value="overview" className="mt-4 grid gap-4 xl:grid-cols-[minmax(0,1.4fr)_minmax(320px,0.6fr)]">
          <div className="space-y-4">
            <SectionCard title={t('investigation_detail.current_conclusion')}>
              <p className="text-sm leading-6 text-tx-1">
                {investigation.summary ?? t('investigation_detail.no_conclusion')}
              </p>
            </SectionCard>
            <SectionCard title={t('investigation_detail.steps')}>
              <StepList steps={steps} />
            </SectionCard>
          </div>
          <SectionCard title={t('investigation_detail.hypotheses')}>
            {hypotheses.length ? (
              <div className="space-y-3">
                {hypotheses.map((hypothesis, index) => (
                  <div key={hypothesis.id} className="rounded-md border border-bd-0 bg-bg-2 p-3">
                    <div className="flex items-center gap-2">
                      <span className="font-mono text-xs text-tx-3">{String.fromCharCode(65 + index)}</span>
                      <StateBadge value={hypothesis.status} />
                      <Badge variant="outline">{t(`confidence.${hypothesis.confidence}`)}</Badge>
                    </div>
                    <p className="mt-2 text-sm text-tx-1">{hypothesis.statement}</p>
                  </div>
                ))}
              </div>
            ) : (
              <EmptyLine text={t('investigation_detail.no_hypotheses')} />
            )}
          </SectionCard>
        </TabsContent>
        <TabsContent value="timeline" className="mt-4">
          <SectionCard title={t('investigation_detail.timeline')}><StepList steps={steps} verbose /></SectionCard>
        </TabsContent>
        <TabsContent value="evidence" className="mt-4">
          <EvidenceList evidence={evidence} />
        </TabsContent>
        <TabsContent value="queries" className="mt-4">
          <EvidenceList evidence={evidence.filter((item) => Boolean(item.query))} queriesOnly />
        </TabsContent>
        <TabsContent value="operations" className="mt-4">
          <ProductState
            variant="empty"
            title={t('investigation_detail.operations_title')}
            description={t('investigation_detail.operations_description')}
            action={<Button asChild><Link to="/intelligence/approvals">{t('nav.approvals')}</Link></Button>}
          />
        </TabsContent>
        <TabsContent value="runs" className="mt-4">
          <ProductState
            variant="empty"
            title={t('investigation_detail.runs_title')}
            description={t('investigation_detail.runs_description')}
            action={<Button asChild><Link to="/intelligence/executions">{t('nav.executions')}</Link></Button>}
          />
        </TabsContent>
      </Tabs>
      <InvestigationEditorDrawer
        target={editor}
        onClose={() => setEditor(null)}
      />
    </ModulePage>
  );
}

function StepList({ steps, verbose = false }: { steps: intelligenceApi.InvestigationStep[]; verbose?: boolean }) {
  const { t } = useTranslation('intelligence');
  if (!steps.length) return <EmptyLine text={t('investigation_detail.no_steps')} />;
  return (
    <ol className="space-y-1">
      {steps.map((step) => (
        <li key={step.id} className="flex gap-3 rounded-md px-2 py-2 hover:bg-bg-2">
          <StepIcon status={step.status} />
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-sm font-strong text-tx-1">{step.title}</span>
              {step.tool_name && <Badge variant="outline" className="font-mono">{step.tool_name}</Badge>}
            </div>
            {verbose && (step.output_summary || step.conclusion_impact || step.error) && (
              <p className={cn('mt-1 text-xs text-tx-3', step.error && 'text-red-soft')}>
                {step.error ?? step.output_summary ?? step.conclusion_impact}
              </p>
            )}
          </div>
          <StateBadge value={step.status} />
        </li>
      ))}
    </ol>
  );
}

function StepIcon({ status }: { status: intelligenceApi.StepStatus }) {
  if (status === 'succeeded') return <Check className="mt-0.5 h-4 w-4 shrink-0 text-green-soft" />;
  if (status === 'failed') return <X className="mt-0.5 h-4 w-4 shrink-0 text-red-soft" />;
  if (status === 'running') return <CircleDot className="mt-0.5 h-4 w-4 shrink-0 text-indigo" />;
  return <Clock3 className="mt-0.5 h-4 w-4 shrink-0 text-tx-3" />;
}

function EvidenceList({
  evidence,
  queriesOnly = false,
}: {
  evidence: intelligenceApi.InvestigationEvidence[];
  queriesOnly?: boolean;
}) {
  const { t } = useTranslation('intelligence');
  if (!evidence.length) {
    return <ProductState variant="empty" title={t('investigation_detail.no_evidence')} />;
  }
  return (
    <div className="grid gap-3 lg:grid-cols-2">
      {evidence.map((item) => (
        <SectionCard key={item.id} title={item.label} badge={<StateBadge value={item.fact_status} />}>
          <p className="text-sm leading-6 text-tx-1">{item.summary}</p>
          {item.query && (
            <pre className="mt-3 max-h-48 overflow-auto rounded-md border border-bd-0 bg-bg-2 p-3 font-mono text-xs text-tx-1">
              {item.query}
            </pre>
          )}
          {!queriesOnly && <p className="mt-2 text-xs text-tx-3">{item.kind}</p>}
        </SectionCard>
      ))}
    </div>
  );
}

export function AutomationsPage() {
  const { t } = useTranslation('intelligence');
  const manageAccess = useActionAccess({
    permission: 'intelligence.manage',
  });
  const [editor, setEditor] = React.useState<AutomationEditorTarget>(null);
  const automations = useQuery({
    queryKey: ['intelligence', 'automations'],
    queryFn: intelligenceApi.listAutomations,
    retry: false,
  });
  const tools = useQuery({
    queryKey: ['intelligence', 'tools'],
    queryFn: intelligenceApi.listTools,
    retry: false,
  });
  const dryRun = useMutation({
    mutationFn: (id: string) => intelligenceApi.dryRunAutomation(id, { type: 'manual.preview' }),
    onSuccess: () => toast.success(t('automations.dry_run_success')),
    onError: (error) => toast.error(String(error)),
  });
  return (
    <ModulePage
      title={t('automations.title')}
      description={t('automations.description')}
      action={
        <Button
          size="sm"
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          onClick={() => setEditor('new')}
        >
          <Plus />{t('automations.create')}
        </Button>
      }
    >
      {automations.isLoading ? (
        <ProductState variant="loading" />
      ) : automations.isError ? (
        <ProductState variant="error" error={automations.error} />
      ) : automations.data?.length ? (
        <div className="grid gap-3 xl:grid-cols-2">
          {automations.data.map((automation) => (
            <SectionCard
              key={automation.id}
              title={automation.name}
              badge={<StateBadge value={automation.enabled ? 'enabled' : 'disabled'} />}
            >
              <p className="text-sm text-tx-2">{automation.description}</p>
              <div className="mt-3 flex flex-wrap gap-1.5">
                {automation.allowed_tools.map((tool) => (
                  <Badge key={tool} variant="outline" className="font-mono">{tool}</Badge>
                ))}
              </div>
              <div className="mt-4 flex items-center justify-between border-t border-bd-0 pt-3">
                <span className="text-xs text-tx-3">{String(automation.trigger.type ?? t('automations.manual'))}</span>
                <div className="flex items-center gap-2">
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={manageAccess.disabled}
                    disabledReason={manageAccess.reason}
                    onClick={() => setEditor(automation)}
                  >
                    <Pencil /> {t('common.edit')}
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={dryRun.isPending}
                    onClick={() => dryRun.mutate(automation.id)}
                  >
                    <FlaskConical /> {t('automations.dry_run')}
                  </Button>
                </div>
              </div>
            </SectionCard>
          ))}
        </div>
      ) : (
        <ProductState
          variant="empty"
          title={t('automations.empty_title')}
          description={t('automations.empty_description')}
          action={
            <Button
              disabled={manageAccess.disabled}
              disabledReason={manageAccess.reason}
              onClick={() => setEditor('new')}
            >
              {t('automations.create')}
            </Button>
          }
        />
      )}
      <AutomationEditorDrawer
        target={editor}
        tools={tools.data?.tools ?? EMPTY_TOOLS}
        onClose={() => setEditor(null)}
      />
    </ModulePage>
  );
}

export function ApprovalsPage() {
  const { t } = useTranslation('intelligence');
  const queryClient = useQueryClient();
  const approveAccess = useActionAccess({
    permission: 'intelligence.approve',
  });
  const approvals = useQuery({
    queryKey: ['intelligence', 'approvals'],
    queryFn: intelligenceApi.listApprovals,
    retry: false,
  });
  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ['intelligence', 'approvals'] }),
      queryClient.invalidateQueries({ queryKey: ['intelligence', 'executions'] }),
      queryClient.invalidateQueries({ queryKey: ['intelligence', 'overview'] }),
    ]);
  };
  const review = useMutation({
    mutationFn: ({ id, approve }: { id: string; approve: boolean }) => {
      if (!approveAccess.allowed) {
        throw new Error(approveAccess.reason);
      }
      return intelligenceApi.reviewApproval(id, approve, '');
    },
    onSuccess: refresh,
    onError: (error) => toast.error(String(error)),
  });
  const execute = useMutation({
    mutationFn: (id: string) => {
      if (!approveAccess.allowed) {
        throw new Error(approveAccess.reason);
      }
      return intelligenceApi.executeApproval(id, `mole-${id}-${Date.now()}`);
    },
    onSuccess: async () => {
      await refresh();
      toast.success(t('approvals.execution_started'));
    },
    onError: (error) => toast.error(String(error)),
  });
  return (
    <ModulePage title={t('approvals.title')} description={t('approvals.description')}>
      {approvals.isLoading ? (
        <ProductState variant="loading" />
      ) : approvals.isError ? (
        <ProductState variant="error" error={approvals.error} />
      ) : approvals.data?.length ? (
        <div className="space-y-3">
          {approvals.data.map((approval) => (
            <article key={approval.id} className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
              <div className="flex items-center gap-3 border-b border-bd-0 bg-bg-2 px-4 py-3">
                <div className="grid h-8 w-8 place-items-center rounded-md border border-indigo/30 bg-indigo/10">
                  <Bot className="h-4 w-4 text-indigo" />
                </div>
                <div className="min-w-0">
                  <h2 className="text-sm font-display-strong text-tx-0">{t('approvals.agent_suggestion')}</h2>
                  <p className="truncate text-xs text-tx-3">{approval.action} · {approval.target}</p>
                </div>
                <div className="ml-auto flex items-center gap-2">
                  <RiskBadge risk={approval.risk} />
                  <StateBadge value={approval.status} />
                </div>
              </div>
              <div className="grid gap-4 p-4 lg:grid-cols-3">
                <LabeledText label={t('approvals.reason')} value={approval.reason} />
                <LabeledText label={t('approvals.impact')} value={approval.impact} />
                <LabeledText
                  label={t('approvals.parameters')}
                  value={JSON.stringify(approval.parameters)}
                  mono
                />
              </div>
              {(approval.status === 'pending' || approval.status === 'approved') && (
                <div className="flex justify-end gap-2 border-t border-bd-0 px-4 py-3">
                  {approval.status === 'pending' ? (
                    <>
                      <Button
                        size="sm"
                        variant="outline"
                        disabled={review.isPending || approveAccess.disabled}
                        disabledReason={
                          !review.isPending ? approveAccess.reason : undefined
                        }
                        onClick={() => review.mutate({ id: approval.id, approve: false })}
                      >
                        <X /> {t('approvals.reject')}
                      </Button>
                      <Button
                        size="sm"
                        disabled={review.isPending || approveAccess.disabled}
                        disabledReason={
                          !review.isPending ? approveAccess.reason : undefined
                        }
                        onClick={() => review.mutate({ id: approval.id, approve: true })}
                      >
                        <ShieldCheck /> {t('approvals.approve')}
                      </Button>
                    </>
                  ) : (
                    <Button
                      size="sm"
                      disabled={execute.isPending || approveAccess.disabled}
                      disabledReason={
                        !execute.isPending ? approveAccess.reason : undefined
                      }
                      onClick={() => execute.mutate(approval.id)}
                    >
                      <Play /> {t('approvals.execute')}
                    </Button>
                  )}
                </div>
              )}
            </article>
          ))}
        </div>
      ) : (
        <ProductState
          variant="empty"
          title={t('approvals.empty_title')}
          description={t('approvals.empty_description')}
        />
      )}
    </ModulePage>
  );
}

export function ExecutionsPage() {
  const { t } = useTranslation('intelligence');
  const executions = useQuery({
    queryKey: ['intelligence', 'executions'],
    queryFn: intelligenceApi.listExecutions,
    retry: false,
  });
  return (
    <ModulePage title={t('executions.title')} description={t('executions.description')}>
      {executions.isLoading ? (
        <ProductState variant="loading" />
      ) : executions.isError ? (
        <ProductState variant="error" error={executions.error} />
      ) : executions.data?.length ? (
        <div className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('executions.columns.operation')}</TableHead>
                <TableHead>{t('executions.columns.target')}</TableHead>
                <TableHead>{t('common.status')}</TableHead>
                <TableHead>{t('executions.columns.verification')}</TableHead>
                <TableHead>{t('common.time')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {executions.data.map((execution) => (
                <TableRow key={execution.id}>
                  <TableCell>
                    <div className="font-strong text-tx-0">{execution.action}</div>
                    <div className="mt-0.5 font-mono text-xs text-tx-3">{execution.idempotency_key}</div>
                  </TableCell>
                  <TableCell>{execution.target}</TableCell>
                  <TableCell><StateBadge value={execution.status} /></TableCell>
                  <TableCell>
                    <StateBadge value={execution.verification.verified === true ? 'verified' : 'unverified'} />
                  </TableCell>
                  <TableCell className="whitespace-nowrap text-xs text-tx-3">
                    {formatMicrosActive(execution.finished_at ?? execution.created_at)}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      ) : (
        <ProductState
          variant="empty"
          title={t('executions.empty_title')}
          description={t('executions.empty_description')}
        />
      )}
    </ModulePage>
  );
}

export function ModulePage({
  title,
  description,
  action,
  backTo,
  children,
}: {
  title: string;
  description?: string;
  action?: React.ReactNode;
  backTo?: string;
  children: React.ReactNode;
}) {
  const { t } = useTranslation('intelligence');
  return (
    <div className="h-full overflow-auto">
      <div className="border-b border-bd-0 bg-bg-1 px-5 py-4">
        {backTo && (
          <Link to={backTo} className="mb-2 inline-flex items-center gap-1 text-xs text-tx-3 hover:text-tx-0">
            <ArrowRight className="h-3.5 w-3.5 rotate-180" /> {t('common.back')}
          </Link>
        )}
        <div className="flex flex-wrap items-end gap-4">
          <div className="min-w-0 flex-1">
            <h1 className="font-sans text-xl font-display-strong tracking-[-0.02em] text-tx-0">{title}</h1>
            {description && <p className="mt-1 max-w-3xl text-sm text-tx-2">{description}</p>}
          </div>
          {action && <div className="ml-auto">{action}</div>}
        </div>
      </div>
      <div className="p-5">{children}</div>
    </div>
  );
}

function PageState({ children }: { children: React.ReactNode }) {
  return <div className="h-full overflow-auto p-5">{children}</div>;
}

function SectionCard({
  title,
  badge,
  children,
}: {
  title: string;
  badge?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-lg border border-bd-0 bg-bg-1">
      <div className="flex min-h-10 items-center gap-2 border-b border-bd-0 px-4 py-2">
        <h2 className="font-sans text-sm font-display-strong text-tx-0">{title}</h2>
        {badge && <div className="ml-auto">{badge}</div>}
      </div>
      <div className="p-4">{children}</div>
    </section>
  );
}

function EmptyLine({ text }: { text: string }) {
  return <div className="rounded-md border border-dashed border-bd-1 px-3 py-6 text-center text-xs text-tx-3">{text}</div>;
}

function LabeledText({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return (
    <div>
      <div className="text-xs font-strong uppercase tracking-[0.08em] text-tx-3">{label}</div>
      <p className={cn('mt-1.5 text-sm leading-6 text-tx-1', mono && 'break-all font-mono text-xs')}>{value || '—'}</p>
    </div>
  );
}

function RiskBadge({ risk }: { risk: intelligenceApi.RiskLevel }) {
  return (
    <Badge
      variant="outline"
      className={cn(
        'font-mono uppercase',
        risk === 'l3' && 'border-red/40 text-red-soft',
        risk === 'l2' && 'border-yellow/40 text-yellow-soft',
        risk === 'l1' && 'border-blue/40 text-blue-soft',
      )}
    >
      {risk}
    </Badge>
  );
}

function StateBadge({ value }: { value: string }) {
  const { t } = useTranslation('intelligence');
  const success = ['completed', 'succeeded', 'supported', 'approved', 'executed', 'verified', 'enabled'];
  const danger = ['failed', 'rejected', 'cancelled', 'verification_failed'];
  const warning = ['pending', 'running', 'waiting_for_data', 'waiting_for_approval', 'testing', 'unverified'];
  return (
    <Badge
      variant="outline"
      className={cn(
        'whitespace-nowrap',
        success.includes(value) && 'border-green/35 bg-green/5 text-green-soft',
        danger.includes(value) && 'border-red/35 bg-red/5 text-red-soft',
        warning.includes(value) && 'border-yellow/35 bg-yellow/5 text-yellow-soft',
      )}
    >
      {t(`status.${value}`, { defaultValue: value })}
    </Badge>
  );
}
