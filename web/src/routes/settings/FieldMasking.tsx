import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { GripVertical } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, PageHeader } from '@/admin';
import * as maskingApi from '@/api/fieldMasking';
import type { StreamType } from '@/api/streams';
import { AlgorithmEditor, algorithmSummary, defaultAlgorithm } from '@/features/fieldMasking/AlgorithmEditor';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState, productStateFor } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shell/ui/table';

import { SectionBody } from './_atoms';

export function FieldMasking() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const access = useActionAccess({ permission: 'org.settings.manage' });
  const [ordered, setOrdered] = React.useState<maskingApi.FieldMaskingRule[]>([]);
  const [draggedId, setDraggedId] = React.useState<string | null>(null);
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] = React.useState<maskingApi.FieldMaskingRule | null>(null);
  const [removing, setRemoving] = React.useState<maskingApi.FieldMaskingRule | null>(null);

  const query = useQuery({
    queryKey: ['field-masking-rules'],
    queryFn: maskingApi.listRules,
  });
  React.useEffect(() => setOrdered(query.data ?? []), [query.data]);

  const reorder = useMutation({
    mutationFn: maskingApi.reorderRules,
    onSuccess: (rows) => {
      setOrdered(rows);
      qc.setQueryData(['field-masking-rules'], rows);
    },
    onError: (error) => {
      toast.error(toApiError(error).message);
      setOrdered(query.data ?? []);
    },
  });
  const remove = useMutation({
    mutationFn: maskingApi.deleteRule,
    onSuccess: () => {
      toast.success(tc('status.deleted'));
      setRemoving(null);
      void qc.invalidateQueries({ queryKey: ['field-masking-rules'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const pageState = productStateFor(
    queryStateFor({ isLoading: query.isLoading, isError: query.isError, data: ordered }),
    {
      error: query.error,
      emptyTitle: t('field_masking.empty_title'),
      emptyDescription: t('field_masking.empty_description'),
    },
  );
  const moveBefore = (targetId: string) => {
    if (!draggedId || draggedId === targetId || access.disabled) return;
    const next = [...ordered];
    const from = next.findIndex((row) => row.id === draggedId);
    const to = next.findIndex((row) => row.id === targetId);
    if (from < 0 || to < 0) return;
    const [moved] = next.splice(from, 1);
    if (!moved) return;
    next.splice(to, 0, moved);
    setOrdered(next);
    setDraggedId(null);
    reorder.mutate(next.map((row) => row.id));
  };

  return (
    <>
      <PageHeader
        title={t('field_masking.title')}
        subtitle={t('field_masking.subtitle')}
        actions={
          <ChromeButton
            variant="primary"
            onClick={() => setCreating(true)}
            disabled={access.disabled}
            disabledReason={access.reason}
          >
            {t('field_masking.new_rule')}
          </ChromeButton>
        }
      />
      <RuleDrawer
        open={creating || editing !== null}
        editing={editing}
        disabled={access.disabled}
        disabledReason={access.reason}
        onClose={() => {
          setCreating(false);
          setEditing(null);
        }}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('field_masking.delete_title')}
        description={removing?.name ?? ''}
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        disabled={access.disabled}
        disabledReason={access.reason}
        onConfirm={() => removing && remove.mutate(removing.id)}
      />
      <SectionBody>
        {pageState ? (
          <ProductState {...pageState} />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-10" />
                <TableHead>{t('field_masking.columns.rule')}</TableHead>
                <TableHead>{t('field_masking.columns.match')}</TableHead>
                <TableHead>{t('field_masking.columns.algorithm')}</TableHead>
                <TableHead className="w-28">{t('field_masking.columns.status')}</TableHead>
                <TableHead className="w-40" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {ordered.map((row) => (
                <TableRow
                  key={row.id}
                  draggable={!access.disabled}
                  onDragStart={(event) => {
                    setDraggedId(row.id);
                    event.dataTransfer.effectAllowed = 'move';
                  }}
                  onDragEnd={() => setDraggedId(null)}
                  onDragOver={(event) => event.preventDefault()}
                  onDrop={() => moveBefore(row.id)}
                  className={draggedId === row.id ? 'opacity-50' : undefined}
                >
                  <TableCell>
                    <GripVertical className="h-4 w-4 cursor-grab text-tx-3" aria-hidden />
                  </TableCell>
                  <TableCell>
                    <div className="font-strong text-tx-0">{row.name}</div>
                  </TableCell>
                  <TableCell>
                    <code className="text-xs text-tx-1">{row.field_pattern}</code>
                    <div className="mt-0.5 text-xs text-tx-3">
                      {[row.stream_type, row.stream_pattern].filter(Boolean).join(' · ') || t('field_masking.all_streams')}
                    </div>
                  </TableCell>
                  <TableCell className="text-xs text-tx-1">
                    {algorithmSummary(row.algorithm, t)}
                  </TableCell>
                  <TableCell className={row.enabled ? 'text-green' : 'text-tx-3'}>
                    {row.enabled ? tc('status.enabled') : tc('status.disabled')}
                  </TableCell>
                  <TableCell>
                    <div className="flex justify-end gap-1">
                      <ChromeButton variant="ghost" size="sm" onClick={() => setEditing(row)} disabled={access.disabled}>
                        {tc('actions.edit')}
                      </ChromeButton>
                      <ChromeButton variant="ghost" size="sm" onClick={() => setRemoving(row)} disabled={access.disabled} className="enabled:hover:text-red-soft">
                        {tc('actions.delete')}
                      </ChromeButton>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </SectionBody>
    </>
  );
}

function RuleDrawer({
  open,
  editing,
  disabled,
  disabledReason,
  onClose,
}: {
  open: boolean;
  editing: maskingApi.FieldMaskingRule | null;
  disabled: boolean;
  disabledReason?: React.ReactNode;
  onClose: () => void;
}) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const [name, setName] = React.useState('');
  const [enabled, setEnabled] = React.useState(true);
  const [fieldPattern, setFieldPattern] = React.useState('');
  const [streamPattern, setStreamPattern] = React.useState('');
  const [streamType, setStreamType] = React.useState('');
  const [algorithm, setAlgorithm] = React.useState<maskingApi.FieldMaskingAlgorithm>(defaultAlgorithm('full'));

  React.useEffect(() => {
    if (!open) return;
    setName(editing?.name ?? '');
    setEnabled(editing?.enabled ?? true);
    setFieldPattern(editing?.field_pattern ?? '');
    setStreamPattern(editing?.stream_pattern ?? '');
    setStreamType(editing?.stream_type ?? '');
    setAlgorithm(editing?.algorithm ?? defaultAlgorithm('full'));
  }, [editing, open]);

  const save = useMutation({
    mutationFn: () => {
      const input: maskingApi.FieldMaskingRuleInput = {
        name,
        enabled,
        field_pattern: fieldPattern,
        stream_pattern: streamPattern || null,
        stream_type: (streamType || null) as StreamType | null,
        algorithm,
      };
      return editing ? maskingApi.updateRule(editing.id, input) : maskingApi.createRule(input);
    },
    onSuccess: () => {
      toast.success(editing ? tc('status.updated') : tc('status.created'));
      void qc.invalidateQueries({ queryKey: ['field-masking-rules'] });
      void qc.invalidateQueries({ queryKey: ['field-masking-effective'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const invalid = !name.trim() || !fieldPattern.trim()
    || (algorithm.kind === 'range' || algorithm.kind === 'outer' ? algorithm.start >= algorithm.end : false);

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={editing ? t('field_masking.edit_title') : t('field_masking.drawer_title')}
      subtitle={t('field_masking.drawer_subtitle')}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          disabled={disabled}
          invalid={invalid}
          disabledReason={disabledReason}
          onCancel={onClose}
          submitLabel={tc('actions.save')}
          formId="field-masking-rule-form"
        />
      }
    >
      <form id="field-masking-rule-form" onSubmit={(event) => { event.preventDefault(); if (!invalid && !disabled) save.mutate(); }}>
        <FormSection title={t('field_masking.section_match')}>
          <FormField label={t('field_masking.field_name')} required>
            <FormInput value={name} onChange={(event) => setName(event.target.value)} disabled={disabled} />
          </FormField>
          <FormField label={t('field_masking.field_pattern')} required hint={t('field_masking.field_pattern_hint')}>
            <FormInput value={fieldPattern} onChange={(event) => setFieldPattern(event.target.value)} placeholder={t('field_masking.field_pattern_placeholder')} disabled={disabled} />
          </FormField>
          <FormField label={t('field_masking.field_stream_pattern')} hint={t('field_masking.field_stream_pattern_hint')}>
            <FormInput value={streamPattern} onChange={(event) => setStreamPattern(event.target.value)} placeholder={t('field_masking.field_stream_pattern_placeholder')} disabled={disabled} />
          </FormField>
          <FormField label={t('field_masking.field_stream_type')}>
            <FormSelect value={streamType} onChange={setStreamType} options={[
              { value: '', label: t('field_masking.all_types') },
              ...(['logs', 'traces', 'profiles', 'extend'] as const).map((value) => ({ value, label: value })),
            ]} disabled={disabled} />
          </FormField>
          <FormField label={t('field_masking.field_enabled')}>
            <Switch checked={enabled} onCheckedChange={setEnabled} disabled={disabled} />
          </FormField>
        </FormSection>
        <FormSection title={t('field_masking.section_algorithm')} className="mb-0">
          <AlgorithmEditor value={algorithm} onChange={setAlgorithm} disabled={disabled} t={t} />
        </FormSection>
      </form>
    </FormDrawer>
  );
}
