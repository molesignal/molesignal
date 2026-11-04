import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { NotifyCard, NotifyCardColor } from '@/api/notify/templates';
import { uiLabelClass } from '@/shell/chrome';
import { FieldArray, FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { Switch } from '@/shell/ui/switch';

import { CARD_COLORS, type CardProblem, MAX_CARD_FIELDS, MAX_CARD_LINKS } from './model';

type Field = NotifyCard['fields'][number];
type Link = NotifyCard['links'][number];

/**
 * The card IM apps and group bots send for an instant messaging template: title, colour, fields, link
 * buttons and a footer; the body above is the card's text. Every input takes placeholders: one focused is where a
 * key picked from the panel goes. Not a `<label>` around the rows: a click on their heading would land
 * in the first one.
 */
export function CardFields({
  on,
  onToggle,
  card,
  setCard,
  problem,
  onFocusInput,
}: {
  on: boolean;
  onToggle: (on: boolean) => void;
  card: NotifyCard;
  setCard: React.Dispatch<React.SetStateAction<NotifyCard>>;
  problem: CardProblem | null;
  /** An input took the focus: `apply` puts a new value into it. */
  onFocusInput: (input: HTMLInputElement, apply: (value: string) => void) => void;
}) {
  const { t } = useTranslation('notify');

  const field = (index: number, key: keyof Field) => (value: string) =>
    setCard((current) => ({
      ...current,
      fields: current.fields.map((item, at) => (at === index ? { ...item, [key]: value } : item)),
    }));

  const link = (index: number, key: keyof Link) => (value: string) =>
    setCard((current) => ({
      ...current,
      links: current.links.map((item, at) => (at === index ? { ...item, [key]: value } : item)),
    }));

  const focus = (apply: (value: string) => void) => (event: React.FocusEvent<HTMLInputElement>) =>
    onFocusInput(event.currentTarget, apply);

  return (
    <section data-card-fields className="space-y-4 rounded-md border border-bd-0 bg-bg-2 p-4">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h3 className="text-xs font-semibold text-tx-0">{t('templates.card.title')}</h3>
          <p className="mt-1 text-xs leading-relaxed text-tx-3">{t('templates.card.description')}</p>
        </div>
        <Switch
          checked={on}
          onCheckedChange={onToggle}
          aria-label={t('templates.card.toggle')}
        />
      </div>
      {on && (
        <>
          <div className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_180px]">
            <FormField label={t('templates.card.card_title')}>
              <FormInput
                placeholder={t('templates.card.card_title_placeholder')}
                value={card.title}
                onFocus={focus((title) => setCard((current) => ({ ...current, title })))}
                onChange={(event) => {
                  const title = event.target.value;
                  setCard((current) => ({ ...current, title }));
                }}
              />
            </FormField>
            <FormField label={t('templates.card.color')}>
              <FormSelect
                value={card.color}
                onChange={(value) =>
                  setCard((current) => ({ ...current, color: value as NotifyCardColor }))
                }
                options={CARD_COLORS.map((value) => ({
                  value,
                  label: t(`templates.card.colors.${value}`),
                }))}
              />
            </FormField>
          </div>
          <div data-card-rows="fields" className="flex flex-col gap-1.5">
            <span className={uiLabelClass}>{t('templates.card.fields')}</span>
            <FieldArray<Field>
              items={card.fields}
              onChange={(fields) => setCard((current) => ({ ...current, fields }))}
              newItem={() => ({ label: '', value: '' })}
              maxItems={MAX_CARD_FIELDS}
              addLabel={t('templates.card.add_field')}
              removeLabel={t('templates.card.remove_field')}
              renderItem={(item, index) => (
                <div className="grid grid-cols-[150px_minmax(0,1fr)] gap-2">
                  <FormInput
                    aria-label={t('templates.card.field_label')}
                    placeholder={t('templates.card.field_label')}
                    value={item.label}
                    onFocus={focus(field(index, 'label'))}
                    onChange={(event) => field(index, 'label')(event.target.value)}
                  />
                  <FormInput
                    aria-label={t('templates.card.field_value')}
                    placeholder="{{labels.service}}"
                    className="font-mono"
                    value={item.value}
                    onFocus={focus(field(index, 'value'))}
                    onChange={(event) => field(index, 'value')(event.target.value)}
                  />
                </div>
              )}
            />
          </div>
          <div data-card-rows="links" className="flex flex-col gap-1.5">
            <span className={uiLabelClass}>{t('templates.card.links')}</span>
            <FieldArray<Link>
              items={card.links}
              onChange={(links) => setCard((current) => ({ ...current, links }))}
              newItem={() => ({ label: '', url: '' })}
              maxItems={MAX_CARD_LINKS}
              addLabel={t('templates.card.add_link')}
              removeLabel={t('templates.card.remove_link')}
              renderItem={(item, index) => (
                <div className="grid grid-cols-[150px_minmax(0,1fr)] gap-2">
                  <FormInput
                    aria-label={t('templates.card.link_label')}
                    placeholder={t('templates.card.link_label')}
                    value={item.label}
                    onFocus={focus(link(index, 'label'))}
                    onChange={(event) => link(index, 'label')(event.target.value)}
                  />
                  <FormInput
                    aria-label={t('templates.card.link_url')}
                    placeholder="{{annotations.runbook_url}}"
                    className="font-mono"
                    value={item.url}
                    onFocus={focus(link(index, 'url'))}
                    onChange={(event) => link(index, 'url')(event.target.value)}
                  />
                </div>
              )}
            />
          </div>
          <FormField label={t('templates.card.note')}>
            <FormInput
              value={card.note}
              onFocus={focus((note) => setCard((current) => ({ ...current, note })))}
              onChange={(event) => {
                const note = event.target.value;
                setCard((current) => ({ ...current, note }));
              }}
            />
          </FormField>
          <p className="text-xs leading-relaxed text-tx-3">{t('templates.card.hint')}</p>
          {problem && (
            <p role="alert" className="text-xs text-red-soft">
              {t(`templates.card.problems.${problem}`)}
            </p>
          )}
        </>
      )}
    </section>
  );
}
