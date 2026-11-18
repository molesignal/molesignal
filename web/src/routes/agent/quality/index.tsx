import { useParams } from 'react-router-dom';

import { QualityAnswers } from './Answers';
import { QualityDataGaps } from './DataGaps';
import { QualityDrills } from './Drills';
import { QualityEvaluators } from './Evaluators';
import { QualityOverview } from './Overview';
import { QualityReviews } from './Reviews';

export { QUALITY_VIEWS, qualityViewOf } from './model';

/**
 * The Quality tab: how good Mole Agent's answers are. Every answer is scored
 * by rules; evaluators the organization sets up score a sample of them with
 * a model; low scores and unhelpful ratings wait for review. The tool calls of
 * the answers also say what is missing from the data (data gaps), and drills
 * investigate resolved incidents again against their known root cause.
 * Holders of `agent.manage` see it: they can read the team chats it is drawn
 * from.
 */
export function AgentQualityPage() {
  const { view } = useParams<{ view?: string }>();

  switch (view) {
    case 'answers':
      return <QualityAnswers />;
    case 'reviews':
      return <QualityReviews />;
    case 'gaps':
      return <QualityDataGaps />;
    case 'drills':
      return <QualityDrills />;
    case 'evaluators':
      return <QualityEvaluators />;
    default:
      return <QualityOverview />;
  }
}
