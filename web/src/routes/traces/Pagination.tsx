import { useTranslation } from 'react-i18next';

import { CursorPagination } from '@/shell/CursorPagination';
import { ResultPagination } from '@/shell/ResultPagination';

export const DEFAULT_TRACE_PAGE_SIZE = 20;
const TRACE_PAGE_SIZE_OPTIONS = [10, 20, 50, 100];

export type TracePaginationModel =
  | {
      kind: 'cursor';
      pageSize: number;
      hasPrevious: boolean;
      hasNext: boolean;
      pending: boolean;
      onPrevious: () => void;
      onNext: () => void;
      onPageSizeChange: (pageSize: number) => void;
    }
  | {
      kind: 'offset';
      page: number;
      pageCount: number;
      pageSize: number;
      onPageChange: (page: number) => void;
      onPageSizeChange: (pageSize: number) => void;
    };

export function TracePagination({ model }: { model: TracePaginationModel }) {
  const { t } = useTranslation('traces');
  if (model.kind === 'cursor') {
    return (
      <CursorPagination
        pageSize={model.pageSize}
        pageSizeOptions={TRACE_PAGE_SIZE_OPTIONS}
        hasPrevious={model.hasPrevious}
        hasNext={model.hasNext}
        pending={model.pending}
        ariaLabel={t('explore.pagination.aria')}
        pageSizeAriaLabel={t('explore.pagination.page_size_aria')}
        previousLabel={t('explore.pagination.previous')}
        nextLabel={t('explore.pagination.next')}
        onPrevious={model.onPrevious}
        onNext={model.onNext}
        onPageSizeChange={model.onPageSizeChange}
      />
    );
  }

  return (
    <ResultPagination
      page={model.page}
      pageCount={model.pageCount}
      pageSize={model.pageSize}
      pageSizeOptions={TRACE_PAGE_SIZE_OPTIONS}
      pageLabel={t('explore.pagination.page_summary', {
        page: model.page,
        pages: model.pageCount,
      })}
      ariaLabel={t('explore.pagination.aria')}
      pageSizeAriaLabel={t('explore.pagination.page_size_aria')}
      firstAriaLabel={t('explore.pagination.first_aria')}
      previousAriaLabel={t('explore.pagination.previous_aria')}
      nextAriaLabel={t('explore.pagination.next_aria')}
      lastAriaLabel={t('explore.pagination.last_aria')}
      onPageChange={model.onPageChange}
      onPageSizeChange={model.onPageSizeChange}
    />
  );
}
