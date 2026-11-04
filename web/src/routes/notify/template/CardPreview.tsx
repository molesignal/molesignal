import { useTranslation } from 'react-i18next';

import type { NotifyCard, NotifyCardColor, NotifyTemplate } from '@/api/notify/templates';

import { TemplateMessagePreview } from './MessagePreview';

type TemplateFormat = NonNullable<NotifyTemplate['format']>;

/** The title band in the colour the title takes: the word in the tone's ink on its tint, as a chip is. */
const HEADERS: Record<Exclude<NotifyCardColor, 'severity'>, string> = {
  blue: 'bg-[var(--blue-tint)] text-[var(--blue-ink)]',
  green: 'bg-[var(--green-tint)] text-[var(--green-ink)]',
  yellow: 'bg-[var(--yellow-tint)] text-[var(--yellow-ink)]',
  orange: 'bg-[var(--orange-tint)] text-[var(--orange-ink)]',
  red: 'bg-red-dim text-red-soft',
  grey: 'bg-bg-3 text-tx-1',
};

/**
 * Roughly how an IM app or group bot shows a card filled with the preview event, drawn as Lark draws
 * it: the title on a band in its colour, the footer under a rule. Each platform draws it in its own
 * card, so this is the shape, not any one of them.
 */
export function CardPreview({
  card,
  color,
  fallbackTitle,
  format,
  body,
}: {
  /** The card as the preview event fills it. */
  card: NotifyCard;
  color: Exclude<NotifyCardColor, 'severity'>;
  /** The notification's own title, for a card without one. */
  fallbackTitle: string;
  format: TemplateFormat;
  body: string;
}) {
  const { t } = useTranslation('notify');

  return (
    <div data-card-preview className="mt-3 overflow-hidden rounded-md bg-bg-1 shadow-sm">
      <div data-card-header={color} className={`px-3 py-2.5 ${HEADERS[color]}`}>
        <h4 className="text-sm font-semibold">{card.title || fallbackTitle}</h4>
      </div>
      <div className="min-w-0 px-3 pb-3">
        <TemplateMessagePreview format={format} content={body} emptyText={t('templates.preview_empty')} />
        {card.fields.length > 0 && (
          <dl className="mt-3 grid grid-cols-2 gap-x-4 gap-y-2">
            {card.fields.map((field, index) => (
              <div key={`${index}:${field.label}`} className="min-w-0">
                <dt className="text-xs font-medium text-tx-2">{field.label}</dt>
                <dd className="break-words text-sm text-tx-1">{field.value}</dd>
              </div>
            ))}
          </dl>
        )}
        {card.links.length > 0 && (
          <div className="mt-3 flex flex-wrap gap-2">
            {card.links.map((link, index) => (
              <a
                key={`${index}:${link.url}`}
                href={link.url}
                target="_blank"
                rel="noreferrer"
                title={link.url}
                className="rounded-md bg-bg-3 px-3 py-1 text-xs font-medium text-tx-1 hover:bg-bg-4"
              >
                {link.label}
              </a>
            ))}
          </div>
        )}
        {card.note && (
          <p className="mt-3 border-t border-bd-0 pt-2 text-xs text-tx-3">{card.note}</p>
        )}
      </div>
    </div>
  );
}
