import { useSearchParams } from 'react-router-dom';

import { isReportRender } from '@/report-render/mode';

import { SavedViews } from '../SavedViews';
import { SavedViewReport } from './report/SavedViewReport';

/**
 * Saved views at their address: the list a person manages them in, or, when
 * the report renderer asks for it, one view's rows alone on a document to print.
 */
export function SavedViewsRoute() {
  const [searchParams] = useSearchParams();

  return isReportRender(searchParams) ? <SavedViewReport /> : <SavedViews />;
}
