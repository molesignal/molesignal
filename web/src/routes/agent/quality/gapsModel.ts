import type { TFunction } from 'i18next';

import type { GapExample, GapGroup, GapKind } from '@/api/agent/quality';

/** What a group of gaps found, in a sentence: the calls the rule counted, on what. */
export function gapNote(t: TFunction, group: GapGroup): string {
  const stream = group.stream ?? t('quality.gaps.no_stream');

  switch (group.rule) {
    case 'repeated_queries':
      return t('quality.gaps.notes.repeated_queries', {
        stream,
        count: group.calls,
        misses: group.misses,
      });
    case 'structure_lookups':
      return group.stream
        ? t('quality.gaps.notes.structure_lookups_stream', { stream, count: group.calls })
        : t('quality.gaps.notes.structure_lookups', { count: group.calls });
    case 'service_mismatch':
      return t('quality.gaps.notes.service_mismatch', { service: group.service ?? '', stream });
    case 'raw_log_pulls':
      return t('quality.gaps.notes.raw_log_pulls', { stream, count: group.calls });
    case 'raw_metric_pulls':
      return t('quality.gaps.notes.raw_metric_pulls', { stream, count: group.calls });
  }
}

/** What fills a gap of this kind. */
export function gapFix(t: TFunction, kind: GapKind): string {
  return t(`quality.gaps.fixes.${kind}`);
}

/** Where an answer that found a gap can be read in full: its chat. */
export function exampleLink(example: Pick<GapExample, 'chatId'>): string | null {
  return example.chatId ? `/agent/chat?chat=${encodeURIComponent(example.chatId)}` : null;
}

/** The share of answers with a gap, 0 to 100; null when no answer called tools. */
export function gapShare(withGaps: number, answers: number): number | null {
  if (answers <= 0) return null;
  return Math.round((Math.min(withGaps, answers) / answers) * 100);
}
