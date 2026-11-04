import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Archive, ArchiveRestore } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { apmApi, apmQueryKeys, type ServiceDefinition, type ServiceLifecycle, type ServiceTier } from '@/api/apm';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { Input } from '@/shell/ui/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/shell/ui/select';
import { Sheet, SheetContent, SheetDescription, SheetTitle } from '@/shell/ui/sheet';
import { toast } from '@/shell/ui/sonner';
import { Textarea } from '@/shell/ui/textarea';
import { useUsers } from '@/shell/useUsers';
import { useAuthStore } from '@/stores/auth';

import { apdexMillisText, formatTags, isRepositoryAddress, parseApdexMillis, parseTags } from './definitionFields';
import { useTeams } from './people';

const NONE = '__none__';
const TIERS: readonly ServiceTier[] = ['p0', 'p1', 'p2', 'p3', 'p4', 'p5'];
const LIFECYCLES: readonly ServiceLifecycle[] = ['production', 'staging', 'experimental', 'deprecated'];

/** Which service the drawer describes; without one it declares a new service. */
export interface DefinitionTarget {
  namespace: string;
  name: string;
  definition: ServiceDefinition | null;
}

/**
 * What people keep about a service: who owns it (a team and a person of the
 * platform's own), how much it matters, where it is in its life, and a line
 * on what it does. A service can be declared before it first reports.
 */
export function DefinitionDrawer({ target, open, onClose }: { target: DefinitionTarget | null; open: boolean; onClose: () => void }) {
  const { t } = useTranslation('apm');

  return (
    <Sheet open={open} onOpenChange={(next) => !next && onClose()}>
      <SheetContent side="right" className="flex w-[440px] max-w-full flex-col gap-0 p-0" data-apm-definition-drawer="">
        <div className="border-b border-bd-0 px-4 py-3">
          <SheetTitle className="font-sans text-sm font-strong text-tx-0">
            {target?.name ? t('definition.title', { name: target.name }) : t('definition.declare_title')}
          </SheetTitle>
          <SheetDescription className="mt-0.5 font-sans text-xs text-tx-2">{t('definition.description')}</SheetDescription>
        </div>
        {/* One form per opening: its fields start from what is kept each time. */}
        {open && <DefinitionForm key={target ? `${target.namespace}/${target.name}` : 'new'} target={target} onDone={onClose} />}
      </SheetContent>
    </Sheet>
  );
}

/**
 * The fields of a service's catalog entry. In a drawer it closes when saved;
 * set into a page (the service's Settings), it stays and keeps its place.
 */
export function DefinitionForm({ target, onDone, embedded = false }: { target: DefinitionTarget | null; onDone: () => void; embedded?: boolean }) {
  const { t } = useTranslation('apm');
  const queryClient = useQueryClient();
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const access = useActionAccess({ permission: 'apm.definitions.manage' });
  const { teams } = useTeams();
  const { users } = useUsers();
  const existing = target?.definition ?? null;
  const [namespace, setNamespace] = React.useState(target?.namespace ?? '');
  const [name, setName] = React.useState(target?.name ?? '');
  const [description, setDescription] = React.useState(existing?.description ?? '');
  const [teamId, setTeamId] = React.useState(existing?.teamId ?? '');
  const [ownerId, setOwnerId] = React.useState(existing?.ownerId ?? '');
  const [tier, setTier] = React.useState<string>(existing?.tier ?? '');
  const [lifecycle, setLifecycle] = React.useState<string>(existing?.lifecycle ?? '');
  const [repository, setRepository] = React.useState(existing?.repositoryUrl ?? '');
  const [tagsText, setTagsText] = React.useState(formatTags(existing?.tags));
  const [apdexText, setApdexText] = React.useState(apdexMillisText(existing?.apdexThreshold));
  const declaring = !target?.name;
  const tags = parseTags(tagsText);
  const apdex = parseApdexMillis(apdexText);
  const valid = Boolean(name.trim()) && tags.problem === null && apdex.valid && isRepositoryAddress(repository);

  const refresh = () =>
    queryClient.invalidateQueries({
      queryKey: apmQueryKeys.definitions(orgId),
    });

  // Archiving saves the form as it stands, with the service archived or not.
  const save = useMutation({
    mutationFn: (archived: boolean) =>
      apmApi.saveDefinition(
        { namespace: namespace.trim() || name.trim(), name: name.trim() },
        {
          description: description.trim(),
          teamId: teamId || null,
          ownerId: ownerId || null,
          tier: (tier || null) as ServiceTier | null,
          lifecycle: (lifecycle || null) as ServiceLifecycle | null,
          repositoryUrl: repository.trim() || null,
          tags: tags.tags,
          apdexThreshold: apdex.micros,
          archived,
        },
      ),
    onSuccess: async (saved) => {
      await refresh();
      toast.success(t(saved.archived === Boolean(existing?.archived) ? 'definition.saved' : saved.archived ? 'definition.archived' : 'definition.unarchived'));
      onDone();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const remove = useMutation({
    mutationFn: () => apmApi.deleteDefinition({ namespace, name }),
    onSuccess: async () => {
      await refresh();
      toast.success(t('definition.removed'));
      onDone();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <form
      className={embedded ? 'flex flex-col' : 'flex min-h-0 flex-1 flex-col'}
      data-apm-definition-form=""
      onSubmit={(event) => {
        event.preventDefault();
        if (valid) save.mutate(Boolean(existing?.archived));
      }}
    >
      <div className={embedded ? 'space-y-4 px-3 py-3 font-sans text-xs' : 'min-h-0 flex-1 space-y-4 overflow-y-auto px-4 py-4 font-sans text-xs'}>
        {declaring && (
          <>
            <Field label={t('definition.fields.name')}>
              <Input value={name} onChange={(event) => setName(event.target.value)} required maxLength={255} data-apm-definition-name="" />
            </Field>
            <Field label={t('definition.fields.namespace')} hint={t('definition.fields.namespace_hint')}>
              <Input value={namespace} onChange={(event) => setNamespace(event.target.value)} maxLength={128} />
            </Field>
          </>
        )}
        <Field label={t('definition.fields.team')}>
          <Choice
            value={teamId}
            onChange={setTeamId}
            none={t('definition.none')}
            options={teams.map((team) => ({
              value: team.id,
              label: team.name,
            }))}
          />
        </Field>
        <Field label={t('definition.fields.owner')} hint={t('definition.fields.owner_hint')}>
          <Choice
            value={ownerId}
            onChange={setOwnerId}
            none={t('definition.none')}
            options={users.map((user) => ({
              value: user.id,
              label: user.name,
            }))}
          />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label={t('definition.fields.tier')}>
            <Choice
              value={tier}
              onChange={setTier}
              none={t('definition.none')}
              options={TIERS.map((value) => ({
                value,
                label: t(`definition.tier.${value}`),
              }))}
            />
          </Field>
          <Field label={t('definition.fields.lifecycle')}>
            <Choice
              value={lifecycle}
              onChange={setLifecycle}
              none={t('definition.none')}
              options={LIFECYCLES.map((value) => ({
                value,
                label: t(`definition.lifecycle.${value}`),
              }))}
            />
          </Field>
        </div>
        <Field
          label={t('definition.fields.repository')}
          hint={t('definition.fields.repository_hint')}
          error={isRepositoryAddress(repository) ? undefined : t('definition.repository_problem')}
        >
          <Input
            inputMode="url"
            value={repository}
            onChange={(event) => setRepository(event.target.value)}
            maxLength={2048}
            placeholder="https://github.com/acme/checkout"
            data-apm-definition-repository=""
          />
        </Field>
        <Field
          label={t('definition.fields.tags')}
          hint={t('definition.fields.tags_hint')}
          error={tags.problem ? t(`definition.tags_problem.${tags.problem}`, { tag: tags.at ?? '' }) : undefined}
        >
          <Input value={tagsText} onChange={(event) => setTagsText(event.target.value)} placeholder="domain=orders, pci" data-apm-definition-tags="" />
        </Field>
        <Field
          label={t('definition.fields.apdex')}
          hint={t('definition.fields.apdex_hint')}
          error={apdex.valid ? undefined : t('definition.apdex_problem')}
        >
          <Input
            type="number"
            inputMode="decimal"
            min={1}
            max={60000}
            value={apdexText}
            onChange={(event) => setApdexText(event.target.value)}
            placeholder="500"
            data-apm-definition-apdex=""
          />
        </Field>
        <Field label={t('definition.fields.description')}>
          <Textarea value={description} onChange={(event) => setDescription(event.target.value)} rows={4} maxLength={2000} />
        </Field>
        <p className="leading-relaxed text-tx-3">
          {t('definition.docs_hint', {
            path: `services/${name.trim() || '…'}/`,
          })}
        </p>
      </div>
      <div className={cn('flex items-center gap-2 border-t border-bd-0', embedded ? 'px-3 py-2' : 'px-4 py-3')}>
        {existing && (
          <Button type="button" variant="ghost" size="sm" disabled={access.disabled || remove.isPending} onClick={() => remove.mutate()} title={access.reason}>
            {t('definition.remove')}
          </Button>
        )}
        {existing && (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={access.disabled || save.isPending || !valid}
            onClick={() => save.mutate(!existing.archived)}
            title={access.reason ?? t(existing.archived ? 'definition.unarchive_hint' : 'definition.archive_hint')}
            data-apm-definition-archive={existing.archived ? 'archived' : 'active'}
          >
            {existing.archived ? <ArchiveRestore aria-hidden className="h-3.5 w-3.5" /> : <Archive aria-hidden className="h-3.5 w-3.5" />}
            {t(existing.archived ? 'definition.unarchive' : 'definition.archive')}
          </Button>
        )}
        <div className="flex-1" />
        {!embedded && (
          <Button type="button" variant="outline" size="sm" onClick={onDone}>
            {t('actions.cancel')}
          </Button>
        )}
        <Button type="submit" size="sm" disabled={access.disabled || save.isPending || !valid} title={access.reason} data-apm-definition-save="">
          {t('actions.save')}
        </Button>
      </div>
    </form>
  );
}

export function Field({ label, hint, error, children }: { label: string; hint?: string; error?: string | undefined; children: React.ReactNode }) {
  return (
    <label className="block space-y-1.5">
      <span className="block font-strong text-tx-1">{label}</span>
      {children}
      {error ? <span className="block text-[var(--red-ink)]">{error}</span> : hint && <span className="block text-tx-3">{hint}</span>}
    </label>
  );
}

export function Choice({
  value,
  onChange,
  none,
  options,
}: {
  value: string;
  onChange: (value: string) => void;
  none: string;
  options: ReadonlyArray<{ value: string; label: string }>;
}) {
  return (
    <Select value={value || NONE} onValueChange={(next) => onChange(next === NONE ? '' : next)}>
      <SelectTrigger className="h-8 w-full text-xs">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={NONE}>{none}</SelectItem>
        {options.map((option) => (
          <SelectItem key={option.value} value={option.value}>
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
