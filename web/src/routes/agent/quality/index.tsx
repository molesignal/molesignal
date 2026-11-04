import { useParams } from 'react-router-dom';

import { QualityAnswers } from './Answers';
import { QualityEvaluators } from './Evaluators';
import { QualityOverview } from './Overview';
import { QualityReviews } from './Reviews';

export { QUALITY_VIEWS, qualityViewOf } from './model';

/**
 * The Quality tab: how good Mole Agent's answers are. Every answer is scored
 * by rules; evaluators the organization sets up score a sample of them with
 * a model; low scores and unhelpful ratings wait for review. Holders of
 * `agent.manage` see it: they can read the team chats it is drawn from.
 */
export function AgentQualityPage() {
  const { view } = useParams<{ view?: string }>();

  switch (view) {
    case 'answers':
      return <QualityAnswers />;
    case 'reviews':
      return <QualityReviews />;
    case 'evaluators':
      return <QualityEvaluators />;
    default:
      return <QualityOverview />;
  }
}
