import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type {
  ComponentStatus,
  StatusPage,
  StatusPageComponent,
  StatusPageComponentInput,
  StatusPageInput,
  StatusPageLanguage,
} from '@/api/statusPages';
import {
  FormChecklist,
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';

import {
  COMPONENT_STATUSES,
  DEFAULT_STATUS_PAGE_HISTORY_DAYS,
  MAX_STATUS_PAGE_HISTORY_DAYS,
  STATUS_PAGE_LANGUAGES,
  componentStatusLabel,
  statusPageLanguages,
} from './model';
import { StatusPageLogoField } from './StatusPageLogoField';

interface PageDraft {
  name: string;
  slug: string;
  logoUrl: string;
  brandColor: string;
  timezone: string;
  language: StatusPageLanguage;
  languages: StatusPageLanguage[];
  historyDays: string;
  deliveryRetentionDays: string;
  privateSessionDays: string;
  visibility: 'public' | 'private';
}

export interface PageFormSubmission {
  input: StatusPageInput;
  logoFile: File | null;
}

const EMPTY_PAGE: PageDraft = {
  name: '',
  slug: '',
  logoUrl: '',
  brandColor: '#4F46E5',
  timezone: 'UTC',
  language: 'en-us',
  languages: ['en-us'],
  historyDays: String(DEFAULT_STATUS_PAGE_HISTORY_DAYS),
  deliveryRetentionDays: '90',
  privateSessionDays: '7',
  visibility: 'public',
};

function pageDraft(page: StatusPage | null): PageDraft {
  if (!page) return EMPTY_PAGE;
  return {
    name: page.name,
    slug: page.slug,
    logoUrl: page.logo_url ?? '',
    brandColor: page.brand_color,
    timezone: page.timezone,
    language: page.language,
    languages: statusPageLanguages(page.language, page.languages),
    historyDays: String(page.history_days ?? DEFAULT_STATUS_PAGE_HISTORY_DAYS),
    deliveryRetentionDays: String(page.delivery_retention_days ?? 90),
    privateSessionDays: String(page.private_session_days ?? 7),
    visibility: page.visibility,
  };
}

function slugify(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 64);
}

export function PageFormDrawer({
  open,
  page,
  busy,
  onOpenChange,
  onSubmit,
}: {
  open: boolean;
  page: StatusPage | null;
  busy: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (submission: PageFormSubmission) => void;
}) {
  const { t } = useTranslation('status-pages');
  const [draft, setDraft] = React.useState<PageDraft>(() => pageDraft(page));
  const [logoFile, setLogoFile] = React.useState<File | null>(null);
  const [slugTouched, setSlugTouched] = React.useState(Boolean(page));
  const formId = 'status-page-form';

  React.useEffect(() => {
    setLogoFile(null);
    if (!open) return;
    setDraft(pageDraft(page));
    setSlugTouched(Boolean(page));
  }, [open, page]);

  const set = <K extends keyof PageDraft>(key: K, value: PageDraft[K]) =>
    setDraft((current) => ({ ...current, [key]: value }));
  const setLanguages = (languages: StatusPageLanguage[]) => {
    if (languages.length === 0) return;
    const ordered = STATUS_PAGE_LANGUAGES.map(({ value }) => value).filter((value) =>
      languages.includes(value),
    );
    const fallbackLanguage = ordered[0];
    if (!fallbackLanguage) return;
    setDraft((current) => ({
      ...current,
      languages: ordered,
      language: ordered.includes(current.language) ? current.language : fallbackLanguage,
    }));
  };
  const invalid =
    !draft.name.trim() ||
    !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(draft.slug) ||
    draft.slug.length < 3 ||
    !/^#[0-9a-fA-F]{6}$/.test(draft.brandColor) ||
    !draft.timezone.trim() ||
    draft.languages.length === 0 ||
    !draft.languages.includes(draft.language) ||
    !Number.isInteger(Number(draft.historyDays)) ||
    Number(draft.historyDays) < 1 ||
    Number(draft.historyDays) > MAX_STATUS_PAGE_HISTORY_DAYS ||
    ![30, 60, 90, 180, 365].includes(Number(draft.deliveryRetentionDays)) ||
    ![1, 7, 30].includes(Number(draft.privateSessionDays));

  return (
    <FormDrawer
      open={open}
      onOpenChange={onOpenChange}
      title={page ? t('forms.page.edit_title') : t('forms.page.create_title')}
      subtitle={t('forms.page.subtitle')}
      footer={
        <FormSubmitFooter
          busy={busy}
          invalid={invalid}
          onCancel={() => onOpenChange(false)}
          submitLabel={page ? t('actions.save') : t('actions.create_page')}
          formId={formId}
        />
      }
    >
      <form
        id={formId}
        onSubmit={(event) => {
          event.preventDefault();
          if (invalid) return;
          onSubmit({
            input: {
              name: draft.name.trim(),
              slug: draft.slug,
              logo_url: logoFile ? page?.logo_url ?? null : draft.logoUrl.trim() || null,
              brand_color: draft.brandColor.toUpperCase(),
              timezone: draft.timezone.trim(),
              language: draft.language,
              languages: draft.languages,
              history_days: Number(draft.historyDays),
              delivery_retention_days: Number(draft.deliveryRetentionDays),
              private_session_days: Number(draft.privateSessionDays),
              visibility: draft.visibility,
            },
            logoFile,
          });
        }}
      >
        <FormSection title={t('forms.page.identity')}>
          <FormField label={t('fields.name')} required>
            <FormInput
              value={draft.name}
              maxLength={120}
              onChange={(event) => {
                const name = event.currentTarget.value;
                setDraft((current) => ({
                  ...current,
                  name,
                  ...(!slugTouched ? { slug: slugify(name) } : {}),
                }));
              }}
              placeholder={t('forms.page.name_placeholder')}
            />
          </FormField>
          <FormField label={t('fields.slug')} hint={t('forms.page.slug_hint')} required>
            <FormInput
              value={draft.slug}
              maxLength={64}
              onChange={(event) => {
                setSlugTouched(true);
                set('slug', slugify(event.currentTarget.value));
              }}
              placeholder="northstar-cloud"
            />
          </FormField>
          <FormField
            label={t('fields.languages')}
            hint={t('forms.page.languages_hint')}
            required
          >
            <FormChecklist
              options={STATUS_PAGE_LANGUAGES}
              selected={draft.languages}
              onChange={setLanguages}
              className="grid grid-cols-1 gap-2 sm:grid-cols-2"
            />
          </FormField>
          <FormRow className="grid-cols-1 sm:grid-cols-2">
            <FormField label={t('fields.default_language')}>
              <FormSelect
                value={draft.language}
                onChange={(value) => set('language', value as StatusPageLanguage)}
                options={STATUS_PAGE_LANGUAGES.filter(({ value }) =>
                  draft.languages.includes(value),
                )}
              />
            </FormField>
            <FormField label={t('fields.visibility')}>
              <FormSelect
                value={draft.visibility}
                onChange={(value) => set('visibility', value as PageDraft['visibility'])}
                options={[
                  { value: 'public', label: t('visibility.public') },
                  { value: 'private', label: t('visibility.private') },
                ]}
              />
            </FormField>
          </FormRow>
          <FormField label={t('fields.timezone')} hint={t('forms.page.timezone_hint')} required>
            <FormInput
              value={draft.timezone}
              onChange={(event) => set('timezone', event.currentTarget.value)}
              placeholder="UTC"
            />
          </FormField>
          <FormRow className="grid-cols-1 sm:grid-cols-2">
            <FormField label={t('fields.delivery_retention_days')}>
              <FormSelect
                value={draft.deliveryRetentionDays}
                onChange={(value) => set('deliveryRetentionDays', value)}
                options={[30, 60, 90, 180, 365].map((days) => ({
                  value: String(days),
                  label: t('values.days', { count: days }),
                }))}
              />
            </FormField>
            <FormField label={t('fields.private_session_days')}>
              <FormSelect
                value={draft.privateSessionDays}
                onChange={(value) => set('privateSessionDays', value)}
                options={[1, 7, 30].map((days) => ({
                  value: String(days),
                  label: t('values.days', { count: days }),
                }))}
              />
            </FormField>
          </FormRow>
          <FormField
            label={t('fields.history_days')}
            hint={t('forms.page.history_days_hint', {
              max: MAX_STATUS_PAGE_HISTORY_DAYS,
            })}
            required
          >
            <FormInput
              type="number"
              inputMode="numeric"
              min={1}
              max={MAX_STATUS_PAGE_HISTORY_DAYS}
              step={1}
              value={draft.historyDays}
              onChange={(event) => set('historyDays', event.currentTarget.value)}
            />
          </FormField>
        </FormSection>

        <FormSection title={t('forms.page.branding')}>
          <StatusPageLogoField
            active={open}
            page={page}
            logoUrl={draft.logoUrl}
            logoFile={logoFile}
            busy={busy}
            onLogoUrlChange={(url) => set('logoUrl', url)}
            onLogoFileChange={setLogoFile}
          />
          <FormField label={t('fields.brand_color')} hint={t('forms.page.color_hint')} required>
            <div className="flex items-center gap-3">
              <span
                aria-hidden
                className="h-9 w-9 shrink-0 rounded-md border border-bd-1"
                style={{ backgroundColor: /^#[0-9a-fA-F]{6}$/.test(draft.brandColor) ? draft.brandColor : 'transparent' }}
              />
              <FormInput
                value={draft.brandColor}
                maxLength={7}
                onChange={(event) => set('brandColor', event.currentTarget.value)}
                placeholder="#4F46E5"
              />
            </div>
          </FormField>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

export function ComponentFormDrawer({
  open,
  component,
  busy,
  onOpenChange,
  onSubmit,
}: {
  open: boolean;
  component: StatusPageComponent | null;
  busy: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (input: StatusPageComponentInput) => void;
}) {
  const { t } = useTranslation('status-pages');
  const [name, setName] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [status, setStatus] = React.useState<ComponentStatus>('operational');
  const [visibility, setVisibility] = React.useState<'enabled' | 'hidden'>('enabled');
  const [lifecycle, setLifecycle] = React.useState<'active' | 'archived'>('active');
  const formId = 'status-page-component-form';

  React.useEffect(() => {
    if (!open) return;
    setName(component?.name ?? '');
    setDescription(component?.description ?? '');
    setStatus(component?.status ?? 'operational');
    setVisibility(component?.visibility ?? 'enabled');
    setLifecycle(component?.lifecycle ?? 'active');
  }, [component, open]);

  return (
    <FormDrawer
      open={open}
      onOpenChange={onOpenChange}
      title={component ? t('forms.component.edit_title') : t('forms.component.create_title')}
      subtitle={t('forms.component.subtitle')}
      footer={
        <FormSubmitFooter
          busy={busy}
          invalid={!name.trim()}
          onCancel={() => onOpenChange(false)}
          submitLabel={component ? t('actions.save') : t('actions.add_component')}
          formId={formId}
        />
      }
    >
      <form
        id={formId}
        onSubmit={(event) => {
          event.preventDefault();
          if (!name.trim()) return;
          onSubmit({
            name: name.trim(),
            description: description.trim(),
            status,
            visibility,
            lifecycle: component ? lifecycle : 'active',
            ...(component ? { position: component.position } : {}),
          });
        }}
      >
        <FormSection>
          <FormField label={t('fields.component_name')} required>
            <FormInput value={name} maxLength={120} onChange={(event) => setName(event.currentTarget.value)} />
          </FormField>
          <FormField label={t('fields.description')}>
            <FormTextarea
              value={description}
              maxLength={500}
              onChange={(event) => setDescription(event.currentTarget.value)}
            />
          </FormField>
          <FormField label={t('fields.component_status')}>
            <FormSelect
              value={status}
              onChange={(value) => setStatus(value as ComponentStatus)}
              options={COMPONENT_STATUSES.map((value) => ({
                value,
                label: componentStatusLabel(t, value),
              }))}
            />
          </FormField>
          <FormRow className={component ? 'grid-cols-1 sm:grid-cols-2' : 'grid-cols-1'}>
            <FormField label={t('fields.component_visibility')}>
              <FormSelect
                value={visibility}
                onChange={(value) => setVisibility(value as 'enabled' | 'hidden')}
                options={[
                  { value: 'enabled', label: t('component_visibility.enabled') },
                  { value: 'hidden', label: t('component_visibility.hidden') },
                ]}
              />
            </FormField>
            {component && (
              <FormField label={t('fields.component_lifecycle')}>
                <FormSelect
                  value={lifecycle}
                  onChange={(value) => setLifecycle(value as 'active' | 'archived')}
                  options={[
                    { value: 'active', label: t('lifecycle.active') },
                    { value: 'archived', label: t('lifecycle.archived') },
                  ]}
                />
              </FormField>
            )}
          </FormRow>
        </FormSection>
      </form>
    </FormDrawer>
  );
}
