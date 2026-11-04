import { useMutation, useQueryClient } from '@tanstack/react-query';
import { FileJson } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { NotifyCategory } from '@/api/notify';
import * as templatesApi from '@/api/notify/templates';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSelect,
  FormTextarea,
  } from '@/shell/FormDrawer';
import { JsonEditor } from '@/shell/JsonEditor';
import { toast } from '@/shell/ui/sonner';

import { NOTIFY_CATEGORIES } from '../model';
import { blockProblem } from './blocks';
import { CardFields } from './CardFields';
import { CardPreview } from './CardPreview';
import { FoldSection } from './FoldSection';
import { TemplateMessagePreview } from './MessagePreview';
import {
  cardColor,
  cardProblem,
  cardToSave,
  characterCount,
  isPlainText,
  notifyTemplatePreview,
  PLAIN_TEXT_LIMIT,
  renderCard,
  renderNotifyTemplate,
  startingCard,
  TEMPLATE_CHANNELS,
  type TemplateChannel,
  templateChannelLabel,
} from './model';
import {
  draftsProblem,
  type ProviderDraft,
  providerDrafts,
  providerProblem,
  providerTemplates,
  TEMPLATE_PROVIDERS,
  type TemplateProvider,
} from './providerDrafts';
import { ProviderTemplateFields } from './ProviderTemplateFields';
import {
  TemplateReferencePanel,
  useTemplateTokenInserter,
} from './ReferencePanel';

type TemplateFormat = NonNullable<templatesApi.NotifyTemplate['format']>;

/** Writes one notify template: its body, what each provider gets, and a preview beside them. */
export function TemplateEditor({
  open,
  template,
  onClose,
}: {
  open: boolean;
  template: templatesApi.NotifyTemplate | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('notify');
  const qc = useQueryClient();
  const [name, setName] = React.useState('');
  const [category, setCategory] =
    React.useState<NotifyCategory>('alert');
  const [channel, setChannel] = React.useState<TemplateChannel>('general');
  const [format, setFormat] = React.useState<TemplateFormat>('text');
  const [body, setBody] = React.useState('');
  const [drafts, setDrafts] = React.useState(() => providerDrafts(undefined));
  const [provider, setProvider] = React.useState<TemplateProvider>('aliyun');
  const draft = drafts[provider];
  const setDraft = (update: ProviderDraft | ((current: ProviderDraft) => ProviderDraft)) =>
    setDrafts((current) => ({
      ...current,
      [provider]: typeof update === 'function' ? update(current[provider]) : update,
    }));
  const [cardOn, setCardOn] = React.useState(false);
  const [card, setCard] = React.useState<templatesApi.NotifyCard>(() => startingCard('alert', t));
  const bodyRef = React.useRef<HTMLTextAreaElement>(null);
  const insertIntoBody = useTemplateTokenInserter(bodyRef, setBody);
  // Placeholders from the panel go where the cursor was last: the body, or an input that said how to
  // take a new value (a provider template variable, a part of the card).
  const insertTarget = React.useRef<{
    input: HTMLInputElement;
    apply: (value: string) => void;
  } | null>(null);
  const insertToken = (token: string) => {
    const target = insertTarget.current;
    if (!target || !target.input.isConnected) {
      insertIntoBody(token);
      return;
    }
    const { input, apply } = target;
    const start = input.selectionStart ?? input.value.length;
    const end = input.selectionEnd ?? input.value.length;
    apply(`${input.value.slice(0, start)}${token}${input.value.slice(end)}`);
    window.requestAnimationFrame(() => {
      input.focus();
      input.setSelectionRange(start + token.length, start + token.length);
    });
  };
  const plainText = isPlainText(channel) ? channel : null;
  const carded = channel === 'im' && cardOn;
  // The event the preview shows: the category's own until a preset written for another is used.
  const [previewEvent, setPreviewEvent] = React.useState<string | undefined>(undefined);
  const [attributes, setAttributes] = React.useState(
    JSON.stringify(
      notifyTemplatePreview('alert').attributes,
      null,
      2,
    ),
  );
  const preview = React.useMemo(() => {
    try {
      const parsed: unknown = JSON.parse(attributes);
      if (!parsed || Array.isArray(parsed) || typeof parsed !== 'object') {
        throw new Error(t('templates.attributes_object'));
      }
      const input = {
        ...notifyTemplatePreview(category, previewEvent),
        attributes: parsed as Record<string, unknown>,
      };
      return {
        rendered: renderNotifyTemplate(body, input),
        params: draft.params
          .filter((param) => param.name.trim() !== '')
          .map((param) => ({
            name: param.name.trim(),
            value: renderNotifyTemplate(param.value, input),
          })),
        card: renderCard(card, input),
        cardColor: cardColor(card.color, input.attributes),
        title: input.messageTitle,
        error: null,
      };
    } catch (error) {
      return {
        rendered: '',
        params: [],
        card: null,
        cardColor: 'blue' as const,
        title: '',
        error: error instanceof Error ? error.message : String(error),
      };
    }
  }, [draft.params, attributes, body, card, category, previewEvent, t]);
  const shownProblem = plainText ? providerProblem(draft) : null;
  // A provider not on show may be the one that stops the save: the fields say which.
  const problem = plainText ? draftsProblem(drafts) : null;
  const cardIssue = carded ? cardProblem(card) : null;
  React.useEffect(() => {
    if (!open) return;
    const nextCategory = template?.category ?? 'alert';
    setName(template?.name ?? '');
    setCategory(nextCategory);
    setChannel(template?.channel ?? 'general');
    setFormat(template?.format ?? 'text');
    setBody(template?.body ?? '');
    setDrafts(providerDrafts(template?.providerTemplates));
    setProvider(
      TEMPLATE_PROVIDERS.find((name) => template?.providerTemplates[name]) ?? 'aliyun',
    );
    setCardOn(Boolean(template?.card));
    setCard(template?.card ?? startingCard(nextCategory, t));
    insertTarget.current = null;
    setPreviewEvent(undefined);
    setAttributes(
      JSON.stringify(
        notifyTemplatePreview(nextCategory).attributes,
        null,
        2,
      ),
    );
  }, [open, template, t]);
  const save = useMutation({
    mutationFn: () => {
      const input: templatesApi.NotifyTemplateInput = {
        name: name.trim(),
        body,
        format: plainText ? 'text' : format,
        category,
        channel: channel === 'general' ? null : channel,
        providerTemplates: plainText ? providerTemplates(drafts) : {},
        card: carded ? cardToSave(card) : null,
      };
      return template
        ? templatesApi.update(template.id, input)
        : templatesApi.create(input);
    },
    onSuccess: () => {
      toast.success(t('common.saved'));
      void qc.invalidateQueries({ queryKey: ['notify', 'templates'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const changeCategory = (nextCategory: NotifyCategory, eventType?: string) => {
    setCategory(nextCategory);
    // A card not turned on yet starts from what this category is about.
    if (!cardOn) setCard(startingCard(nextCategory, t));
    setPreviewEvent(eventType);
    setAttributes(
      JSON.stringify(
        notifyTemplatePreview(nextCategory, eventType).attributes,
        null,
        2,
      ),
    );
  };
  const changeChannel = (next: TemplateChannel) => {
    setChannel(next);
    // An SMS or a call is one plain text; only a webhook's body is JSON.
    if (isPlainText(next) || (next !== 'webhook' && format === 'json')) setFormat('text');
  };
  const usePreset = (preset: templatesApi.NotifyTemplatePreset) => {
    // The preview shows the event the preset is written for: a resolved alert for a resolved notice.
    changeCategory(preset.category, preset.eventType);
    setFormat(preset.format);
    setBody(preset.body);
    window.requestAnimationFrame(() => {
      bodyRef.current?.focus();
      bodyRef.current?.setSelectionRange(
        preset.body.length,
        preset.body.length,
      );
    });
  };
  const length = plainText ? characterCount(preview.rendered) : 0;

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      width={1180}
      title={
        template
          ? t('templates.edit_title', { name: template.name })
          : t('templates.new_title')
      }
      subtitle={t('templates.editor_subtitle')}
      footer={
        <>
          <ChromeButton onClick={onClose}>{t('common.cancel')}</ChromeButton>
          <ChromeButton
            variant="primary"
            disabled={
              !name.trim() ||
              !body.trim() ||
              preview.error !== null ||
              problem !== null ||
              cardIssue !== null ||
              save.isPending
            }
            onClick={() => save.mutate()}
          >
            {save.isPending ? t('common.saving') : t('common.save')}
          </ChromeButton>
        </>
      }
    >
      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_minmax(390px,1fr)]">
        <div className="space-y-4">
          <FormField label={t('templates.name')} required>
            <FormInput
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
          </FormField>
          <div className={`grid gap-4 ${plainText ? 'sm:grid-cols-2' : 'sm:grid-cols-3'}`}>
            <FormField label={t('templates.category')}>
              <FormSelect
                value={category}
                onChange={(value) =>
                  changeCategory(value as NotifyCategory)
                }
                options={NOTIFY_CATEGORIES.map((value) => ({
                  value,
                  label: t(`preferences.${value}`),
                }))}
              />
            </FormField>
            <FormField label={t('templates.channel')}>
              <FormSelect
                value={channel}
                onChange={(value) => changeChannel(value as TemplateChannel)}
                options={TEMPLATE_CHANNELS.map((value) => ({
                  value,
                  label: templateChannelLabel(t, value),
                }))}
              />
            </FormField>
            {!plainText && (
              <FormField label={t('templates.format')}>
                <FormSelect
                  value={format}
                  onChange={(value) => setFormat(value as TemplateFormat)}
                  options={channel === 'webhook' ? ['text', 'markdown', 'html', 'json'] : ['text', 'markdown', 'html']}
                />
              </FormField>
            )}
          </div>
          <FormField
            label={t('templates.body')}
            hint={
              plainText
                ? t(`templates.body_hint_${plainText}`)
                : t('templates.variables_hint')
            }
            required
          >
            <FormTextarea
              ref={bodyRef}
              className={`${plainText ? 'min-h-32' : 'min-h-72'} font-mono text-xs`}
              value={body}
              onFocus={() => {
                insertTarget.current = null;
              }}
              onChange={(event) => setBody(event.target.value)}
            />
          </FormField>
          {blockProblem(body) && (
            <p data-template-block-problem className="-mt-2 font-sans text-xs text-[var(--red-ink)]">
              {t('templates.conditions.problem', { problem: blockProblem(body) })}
            </p>
          )}
          {plainText && (
            <ProviderTemplateFields
              channel={plainText}
              provider={provider}
              onProvider={setProvider}
              draft={draft}
              problem={shownProblem}
              problemElsewhere={problem && problem.provider !== provider ? problem.provider : null}
              onChange={setDraft}
              onFocusValue={(input, index) => {
                insertTarget.current = {
                  input,
                  apply: (value) =>
                    setDraft((current) => ({
                      ...current,
                      params: current.params.map((param, at) =>
                        at === index ? { ...param, value } : param,
                      ),
                    })),
                };
              }}
            />
          )}
          {channel === 'im' && (
            <CardFields
              on={cardOn}
              onToggle={setCardOn}
              card={card}
              setCard={setCard}
              problem={cardIssue}
              onFocusInput={(input, apply) => {
                insertTarget.current = { input, apply };
              }}
            />
          )}
        </div>
        <div className="space-y-4">
          <TemplateReferencePanel
            category={category}
            channel={channel}
            onInsert={insertToken}
            onUsePreset={usePreset}
          />
          <FoldSection icon={FileJson} title={t('templates.preview_attributes')}>
            <div className="p-3">
              <JsonEditor
                value={attributes}
                onChange={setAttributes}
                ariaLabel={t('templates.preview_attributes')}
                minHeight={192}
                maxHeight={420}
              />
            </div>
          </FoldSection>
          <section className="rounded-md border border-bd-0 bg-bg-2 p-4">
            <h3 className="text-xs font-semibold tracking-normal text-tx-2">
              {t('templates.preview')}
            </h3>
            {preview.error ? (
              <p className="mt-3 text-xs text-red-soft">{preview.error}</p>
            ) : (
              <>
                {carded && preview.card ? (
                  <CardPreview
                    card={preview.card}
                    color={preview.cardColor}
                    fallbackTitle={preview.title}
                    // A card shows its text as Markdown in every app, a plain-text body too.
                    format={format === 'html' ? 'html' : 'markdown'}
                    body={preview.rendered}
                  />
                ) : (
                  <TemplateMessagePreview
                    format={plainText ? 'text' : format}
                    content={preview.rendered}
                    emptyText={t('templates.preview_empty')}
                  />
                )}
                {plainText && preview.rendered !== '' && (
                  <p
                    data-template-length
                    className={`mt-2 text-xs ${
                      length > PLAIN_TEXT_LIMIT[plainText] ? 'text-red-soft' : 'text-tx-3'
                    }`}
                  >
                    {t(
                      length > PLAIN_TEXT_LIMIT[plainText]
                        ? `templates.length_over_${plainText}`
                        : 'templates.length',
                      { chars: length, limit: PLAIN_TEXT_LIMIT[plainText] },
                    )}
                  </p>
                )}
                {plainText && draft.code.trim() !== '' && (
                  <div data-provider-preview className="mt-4 border-t border-bd-0 pt-3">
                    <h4 className="text-xs font-semibold text-tx-2">
                      {t('templates.provider.preview', {
                        provider: t(`templates.provider.names.${provider}`),
                        code: draft.code.trim(),
                      })}
                    </h4>
                    <dl className="mt-2 space-y-1 text-xs">
                      {preview.params.map((param) => (
                        <div key={param.name} className="flex min-w-0 gap-2">
                          <dt className="shrink-0 font-mono text-tx-2">{param.name}</dt>
                          <dd className="min-w-0 break-words text-tx-1">{param.value}</dd>
                        </div>
                      ))}
                    </dl>
                  </div>
                )}
              </>
            )}
          </section>
        </div>
      </div>
    </FormDrawer>
  );
}
