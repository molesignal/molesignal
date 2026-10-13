import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { Source } from './sources';

type SourceRef = Pick<Source, 'id'>;

export interface SourceStepText {
  title: string;
  description?: string;
  note?: string;
}

export interface SourceText {
  name: (source: SourceRef) => string;
  description: (source: SourceRef) => string;
  /**
   * The words of one step, counted from zero like `source.steps`. `slots` fill
   * the `{{ENDPOINT}}` a sentence may name, with this workspace's address.
   */
  step: (source: SourceRef, index: number, slots?: Record<string, string>) => SourceStepText;
}

/**
 * The catalogue in the reader's language. `sources.ts` keeps what does not
 * translate (identifiers, signals, commands); the words live in the
 * `datasources` namespace under the source's id.
 */
export function useSourceText(): SourceText {
  const { t, i18n } = useTranslation('datasources');

  return React.useMemo(() => {
    const optional = (key: string, slots: Record<string, string>) =>
      i18n.exists(key, { ns: 'datasources' }) ? t(key, slots) : undefined;

    return {
      name: (source) => t(`${source.id}.name`),
      description: (source) => t(`${source.id}.description`),
      step: (source, index, slots = {}) => {
        const base = `${source.id}.steps.${index + 1}`;
        const description = optional(`${base}.description`, slots);
        const note = optional(`${base}.note`, slots);
        return {
          title: t(`${base}.title`, slots),
          ...(description ? { description } : {}),
          ...(note ? { note } : {}),
        };
      },
    };
  }, [i18n, t]);
}
