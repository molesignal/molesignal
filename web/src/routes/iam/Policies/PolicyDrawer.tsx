import { useMutation, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { Copy } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as policiesApi from '@/api/policies';
import { toApiError } from '@/lib/http';
import { useSessionPermissionScope } from '@/product/access';
import type { ActionAccess } from '@/product/actionAccess';
import {
  type PermissionCatalog,
  type PermissionKey,
  permissionDefinitionsByDomain,
  permissionDefinitionsOf,
} from '@/product/permissions';
import { CopyableValue, ReadOnlyValue } from '@/routes/settings/_atoms';
import { ChromeButton, uiLabelClass } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { JsonEditor } from '@/shell/JsonEditor';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { toast } from '@/shell/ui/sonner';

import { PermissionList, PermissionPicker, permissionGroupsFor } from '../PermissionPicker';
import {
  type PolicyProblem,
  documentAllowing,
  problemMarkers,
  readPolicy,
} from './document';
import { policyDescription, policyName } from './labels';

/** What the drawer is open for: a new policy, one to edit, read or copy. */
export type PolicyDrawerMode =
  | { kind: 'create' }
  | { kind: 'edit'; policy: policiesApi.IamPolicy }
  | { kind: 'view'; policy: policiesApi.IamPolicy }
  | { kind: 'copy'; policy: policiesApi.IamPolicy };

type Editor = 'visual' | 'json';

const KEY_PATTERN = /^[a-z][a-z0-9_]{1,63}$/;

/**
 * A policy's name and what it allows, ticked from the catalog or written as a
 * JSON document. Both edit the same document: ticking rewrites it as one
 * statement, so a document with patterns or several statements opens as JSON.
 */
export function PolicyDrawer({
  mode,
  catalog,
  policies,
  access,
  onClose,
  onCopy,
}: {
  mode: PolicyDrawerMode | null;
  catalog: PermissionCatalog | undefined;
  policies: readonly policiesApi.IamPolicy[];
  access: ActionAccess;
  onClose: () => void;
  onCopy: (policy: policiesApi.IamPolicy) => void;
}) {
  const { t } = useTranslation('iam');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const [key, setKey] = React.useState('');
  const [name, setName] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [text, setText] = React.useState('');
  const [editor, setEditor] = React.useState<Editor>('visual');
  const [keyEdited, setKeyEdited] = React.useState(false);
  const [original, setOriginal] = React.useState('');

  const open = mode !== null;
  const policy = mode && mode.kind !== 'create' ? mode.policy : undefined;
  const readOnly = mode?.kind === 'view';
  const isEdit = mode?.kind === 'edit';
  const isNew = mode?.kind === 'create' || mode?.kind === 'copy';

  const scope = useSessionPermissionScope();
  const allowable = React.useMemo(
    () => (catalog ? permissionDefinitionsOf(catalog, scope).map((permission) => permission.key) : []),
    [catalog, scope],
  );
  const reading = React.useMemo(() => readPolicy(text, allowable), [text, allowable]);

  React.useEffect(() => {
    if (!mode) return;

    const source = mode.kind === 'create' ? undefined : mode.policy;
    const document = source ? JSON.stringify(source.document, null, 2) : documentAllowing([]);
    const simple = source ? readPolicy(document, allowable) : undefined;

    setKey(mode.kind === 'copy' ? copyKey(mode.policy.key) : (source?.key ?? ''));
    setName(
      mode.kind === 'copy'
        ? t('policies.copy_name', { name: policyName(mode.policy, t) })
        : source
          ? policyName(source, t)
          : '',
    );
    setDescription(source ? policyDescription(source, t) : '');
    setText(document);
    setOriginal(document);
    setEditor(simple && !(simple.ok && simple.simple) ? 'json' : 'visual');
    setKeyEdited(mode.kind !== 'create');
    // Opening reads the policy once; the catalog arriving later must not reset a draft.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode]);

  React.useEffect(() => {
    if (!open || !isNew || keyEdited) return;
    setKey(policyKeyFromName(name));
  }, [isNew, keyEdited, name, open]);

  const save = useMutation({
    mutationFn: () => {
      const document = JSON.parse(text) as unknown;

      if (mode?.kind === 'edit') {
        return policiesApi.update(mode.policy.id, { name, description, document });
      }

      return policiesApi.create({ key, name, description, document });
    },
    onSuccess: async () => {
      toast.success(isEdit ? t('policies.toast_updated') : t('policies.toast_created'));
      onClose();
      await Promise.all([
        qc.invalidateQueries({ queryKey: policiesApi.POLICIES_QUERY_KEY }),
        qc.invalidateQueries({ queryKey: ['iam', 'roles'] }),
      ]);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const keyTaken = isNew && policies.some((candidate) => candidate.key === key.trim());
  const keyInvalid = isNew && !KEY_PATTERN.test(key.trim());
  const fieldsInvalid = keyInvalid || keyTaken || name.trim().length === 0 || !reading.ok;
  // Nothing allowed yet is where a policy starts: it holds back the save, not the editors.
  const allowsNothing = reading.ok && reading.permissions.length === 0;
  const invalid = fieldsInvalid || allowsNothing;
  const dirty =
    !isEdit ||
    !policy ||
    name !== policy.name ||
    description !== policy.description ||
    text !== original;
  const controlsDisabled = readOnly || access.disabled || save.isPending;
  const submitDisabled = access.disabled || !dirty;

  const problem = reading.ok ? undefined : describeProblem(reading.problem, t);
  const markers = !reading.ok && problem ? problemMarkers(text, reading.problem, problem) : [];
  const allowed = reading.ok ? reading.permissions : [];

  const title =
    mode?.kind === 'edit'
      ? t('policies.drawer_edit_title')
      : mode?.kind === 'view'
        ? t('policies.drawer_view_title')
        : t('policies.drawer_create_title');

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={title}
      subtitle={t('policies.drawer_subtitle')}
      footer={
        readOnly && policy ? (
          <>
            <ChromeButton type="button" onClick={onClose}>
              {tc('actions.close')}
            </ChromeButton>
            <ChromeButton
              type="button"
              variant="primary"
              disabled={access.disabled}
              disabledReason={access.reason}
              onClick={() => onCopy(policy)}
            >
              <Copy className="h-3 w-3" />
              {t('policies.copy')}
            </ChromeButton>
          </>
        ) : (
          <FormSubmitFooter
            busy={save.isPending}
            disabled={submitDisabled}
            invalid={invalid}
            disabledReason={
              access.reason ??
              (keyTaken
                ? t('policies.key_taken')
                : fieldsInvalid
                  ? tc('access.form_invalid')
                  : allowsNothing
                    ? t('policies.allow_one')
                    : !dirty
                      ? tc('access.no_changes')
                      : undefined)
            }
            onCancel={onClose}
            submitLabel={isEdit ? t('policies.save_policy') : t('policies.create_policy')}
            formId="policy-form"
          />
        )
      }
    >
      <form
        id="policy-form"
        data-policy-form
        onSubmit={(event) => {
          event.preventDefault();
          if (readOnly || submitDisabled || invalid || save.isPending) return;
          save.mutate();
        }}
      >
        {readOnly && policy?.builtin && (
          <p className="mb-6 rounded-md bg-bg-2 px-3 py-2 font-sans text-xs text-tx-2">
            {t('policies.builtin_read_only')}
          </p>
        )}
        <FormSection title={t('policies.sections.identity')}>
          {readOnly ? (
            <ReadOnlyField label={t('policies.fields.name')}>
              <ReadOnlyValue value={name} />
            </ReadOnlyField>
          ) : (
            <FormField label={t('policies.fields.name')} required>
              <FormInput
                value={name}
                onChange={(event) => setName(event.target.value)}
                disabled={controlsDisabled}
                disabledReason={access.reason}
                required
              />
            </FormField>
          )}
          {isNew ? (
            <FormField
              label={t('policies.fields.key')}
              required
              hint={keyTaken ? t('policies.key_taken') : t('policies.fields.key_hint')}
            >
              <FormInput
                value={key}
                onChange={(event) => {
                  setKeyEdited(true);
                  setKey(event.target.value);
                }}
                disabled={controlsDisabled}
                disabledReason={access.reason}
                required
              />
            </FormField>
          ) : (
            policy && (
              // What an API token's `policy_id` names.
              <ReadOnlyField label={t('policies.fields.id')} hint={t('policies.fields.id_hint')}>
                <CopyableValue
                  value={policy.id}
                  copyLabel={t('policies.copy_id')}
                  copiedLabel={tc('actions.copied')}
                />
              </ReadOnlyField>
            )
          )}
          {readOnly ? (
            <ReadOnlyField label={t('policies.fields.description')}>
              <p className="m-0 rounded-md bg-[var(--page-canvas)] px-3 py-2 font-sans text-sm leading-relaxed text-tx-1">
                {description || '—'}
              </p>
            </ReadOnlyField>
          ) : (
            <FormField label={t('policies.fields.description')}>
              <FormTextarea
                value={description}
                onChange={(event) => setDescription(event.target.value)}
                rows={2}
                disabled={controlsDisabled}
                disabledReason={access.reason}
              />
            </FormField>
          )}
        </FormSection>

        <FormSection
          title={t('policies.sections.permissions')}
          description={t('policies.sections.permissions_description')}
        >
          <ViewTabs<Editor>
            label={t('policies.editor_aria')}
            value={editor}
            onValueChange={setEditor}
            items={[
              { key: 'visual', label: t('policies.editor_visual') },
              { key: 'json', label: t('policies.editor_json') },
            ]}
          />

          {editor === 'visual' ? (
            reading.ok ? (
              <>
                {!reading.simple && !readOnly && (
                  <p className="m-0 font-sans text-xs text-[var(--orange-ink)]">
                    {t('policies.visual_rewrites')}
                  </p>
                )}
                {readOnly ? (
                  <PermissionList groups={permissionGroupsFor(catalog, allowed, scope)} />
                ) : (
                  <PermissionPicker
                    groups={permissionDefinitionsByDomain(catalog, scope)}
                    selected={allowed as PermissionKey[]}
                    onChange={(next) => setText(documentAllowing(next))}
                    disabled={controlsDisabled}
                    disabledReason={access.reason}
                  />
                )}
              </>
            ) : (
              <p role="alert" className="m-0 font-sans text-xs text-red-soft">
                {t('policies.visual_needs_valid_json', { problem })}
              </p>
            )
          ) : (
            <>
              <JsonEditor
                value={text}
                {...(readOnly ? {} : { onChange: setText })}
                readOnly={readOnly}
                ariaLabel={t('policies.document_aria')}
                minHeight={240}
                maxHeight={520}
                markers={markers}
                showProblems
              />
              {problem ? (
                <p role="alert" className="m-0 font-sans text-xs text-red-soft" data-policy-problem>
                  {problem}
                </p>
              ) : (
                <>
                  <p className="m-0 font-sans text-xs text-tx-2" data-policy-allowed={allowed.length}>
                    {allowsNothing
                      ? t('policies.allows_none')
                      : t('policies.allows', { count: allowed.length })}
                  </p>
                  {!allowsNothing && <PermissionList groups={permissionGroupsFor(catalog, allowed, scope)} />}
                </>
              )}
            </>
          )}
        </FormSection>
      </form>
    </FormDrawer>
  );
}

/** A value to read, labelled the way a field is, without a `<label>` to steal its clicks. */
function ReadOnlyField({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <span className={uiLabelClass}>{label}</span>
      {children}
      {hint && <span className="font-sans text-xs leading-relaxed text-tx-3">{hint}</span>}
    </div>
  );
}

/** What is wrong with a document, in the interface language. */
function describeProblem(problem: PolicyProblem, t: TFunction<'iam'>): string {
  return t(`policies.problems.${problem.code}`, {
    path: problem.path ?? '',
    value: problem.value ?? '',
  });
}

function copyKey(key: string): string {
  return `${key}_copy`.slice(0, 64);
}

function policyKeyFromName(name: string): string {
  const normalized = name
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '');
  const prefixed = /^[a-z]/.test(normalized) ? normalized : `policy_${normalized}`;
  return prefixed.replace(/_+$/, '').slice(0, 64) || 'policy';
}
