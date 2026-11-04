import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as qualityApi from '@/api/agent/quality';
import type { AnswerDetail, ScoredAnswer } from '@/api/agent/quality';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { InspectionReport } from '@/routes/logs/inspection/Report';
import { FormDrawer } from '@/shell/FormDrawer';

import { answerLink, dimensionNote, evaluatorName, overallScore } from './model';
import { ReviewPanel } from './ReviewPanel';
import { ScoreWord } from './ScoreWord';
import { QUALITY_KEY } from './state';

/**
 * One scored answer: the question and the whole answer, what the rules found
 * in each dimension, what each evaluator said, and its review.
 */
export function AnswerDrawer({
  answer,
  threshold,
  onClose,
}: {
  answer: Pick<ScoredAnswer, 'targetKind' | 'targetId' | 'question'> | null;
  threshold: number;
  onClose: () => void;
}) {
  const { t } = useTranslation('agent');
  const detail = useQuery({
    queryKey: [...QUALITY_KEY, 'answer', answer?.targetKind, answer?.targetId],
    queryFn: () => qualityApi.getAnswer(answer!.targetKind, answer!.targetId),
    enabled: answer !== null,
    retry: false,
  });

  if (!answer) return null;

  return (
    <FormDrawer
      open
      onOpenChange={(open) => !open && onClose()}
      title={t('quality.answer.title')}
      subtitle={answer.question || undefined}
      width={720}
    >
      {detail.isPending ? (
        <ProductState variant="loading" />
      ) : detail.isError ? (
        <ProductState variant="error" error={detail.error} />
      ) : (
        <Detail detail={detail.data} threshold={threshold} />
      )}
    </FormDrawer>
  );
}

function Detail({ detail, threshold }: { detail: AnswerDetail; threshold: number }) {
  const { t } = useTranslation('agent');
  const { answer, material, review, evaluators } = detail;
  const rules = answer.scores.find((score) => score.scorer === 'rules');
  const evaluated = answer.scores.filter((score) => score.scorer !== 'rules');
  const link = answerLink(answer);

  return (
    <div data-quality-detail={answer.targetId} className="flex flex-col gap-5 font-sans text-xs">
      <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
        <span className="text-tx-2">{t('quality.columns.score')}</span>
        <ScoreWord score={overallScore(answer)} threshold={threshold} className="text-sm" />
        <span className="text-tx-1">{t(`quality.sources.${answer.source}`)}</span>
        <span className="text-tx-3">{formatMicrosActive(answer.targetAt)}</span>
        {link && (
          <Link to={link} className="ml-auto text-[var(--blue-ink)] hover:text-tx-0">
            {answer.targetKind === 'incident_rca' ? t('quality.answer.open_incident') : t('quality.answer.open_chat')}
          </Link>
        )}
      </div>

      {review && <ReviewPanel review={review} />}

      <section aria-label={t('quality.answer.evaluators')} className="flex flex-col gap-2">
        <h3 className="m-0 text-xs font-strong text-tx-1">{t('quality.answer.evaluators')}</h3>
        {evaluated.length === 0 ? (
          <p className="m-0 text-tx-3">{t('quality.answer.not_evaluated')}</p>
        ) : (
          evaluated.map((score) => (
            <div key={score.scorer} data-quality-evaluator-score={score.scorer} className="flex flex-col gap-0.5">
              <div className="flex flex-wrap items-baseline gap-2">
                <span className="text-tx-1">{evaluatorName(t, evaluators, score.scorer)}</span>
                <ScoreWord score={score.score} threshold={threshold} />
                {score.samples > 1 && (
                  <span className="text-tx-3">{t('quality.answer.samples', { count: score.samples })}</span>
                )}
                {score.evaluatorModel && <span className="text-tx-3">{score.evaluatorModel}</span>}
              </div>
              {score.reason && <p className="m-0 leading-relaxed text-tx-1">{score.reason}</p>}
            </div>
          ))
        )}
      </section>

      {rules && (
        <section aria-label={t('quality.answer.rules')} className="flex flex-col gap-1.5">
          <div className="flex items-baseline gap-2">
            <h3 className="m-0 text-xs font-strong text-tx-1">{t('quality.answer.rules')}</h3>
            <ScoreWord score={rules.score} threshold={threshold} />
          </div>
          <ul className="m-0 flex list-none flex-col gap-1 p-0">
            {rules.dimensions.map((dimension) => (
              <li key={dimension.dimension} data-quality-rule={dimension.dimension} className="flex items-baseline gap-2">
                <span className="w-[112px] shrink-0 text-tx-2">
                  {t(`quality.dimensions.${dimension.dimension}.title`)}
                </span>
                <ScoreWord score={dimension.score} threshold={threshold} className="w-[28px] shrink-0" />
                <span className="min-w-0 text-tx-1">{dimensionNote(t, dimension)}</span>
              </li>
            ))}
          </ul>
        </section>
      )}

      {material ? (
        <>
          <section aria-label={t('quality.answer.question')} className="flex flex-col gap-1">
            <h3 className="m-0 text-xs font-strong text-tx-1">{t('quality.answer.question')}</h3>
            <p className="m-0 whitespace-pre-wrap leading-relaxed text-tx-0">{material.question}</p>
          </section>
          {material.tools.length > 0 && (
            <p className="m-0 text-tx-2">
              {t('quality.answer.tools', {
                count: material.tools.length,
                failed: material.tools.filter((tool) => tool.failed).length,
              })}
            </p>
          )}
          <section aria-label={t('quality.answer.answer')} className="flex flex-col gap-1">
            <h3 className="m-0 text-xs font-strong text-tx-1">{t('quality.answer.answer')}</h3>
            <InspectionReport content={material.answer} />
          </section>
        </>
      ) : (
        <p className="m-0 text-tx-3">{t('quality.answer.gone')}</p>
      )}
    </div>
  );
}
