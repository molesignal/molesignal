import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ResultPagination } from '@/shell/ResultPagination';

const REPORT_PAGE_SIZE_OPTIONS = [20, 50, 100];
const DEFAULT_REPORT_PAGE_SIZE = 20;

export function useReportPagination<T>(
  items: readonly T[],
  resetKey: string,
) {
  const [page, setPage] = React.useState(1);
  const [pageSize, setPageSize] = React.useState(DEFAULT_REPORT_PAGE_SIZE);
  const pageCount = Math.max(1, Math.ceil(items.length / pageSize));

  React.useEffect(() => {
    setPage(1);
  }, [resetKey]);

  React.useEffect(() => {
    setPage((current) => Math.min(current, pageCount));
  }, [pageCount]);

  const pageItems = React.useMemo(() => {
    const start = (page - 1) * pageSize;
    return items.slice(start, start + pageSize);
  }, [items, page, pageSize]);

  const changePageSize = React.useCallback((nextPageSize: number) => {
    setPageSize(nextPageSize);
    setPage(1);
  }, []);

  return {
    page,
    pageCount,
    pageSize,
    pageItems,
    total: items.length,
    onPageChange: setPage,
    onPageSizeChange: changePageSize,
  };
}

export function ReportPagination({
  page,
  pageCount,
  pageSize,
  total,
  onPageChange,
  onPageSizeChange,
}: {
  page: number;
  pageCount: number;
  pageSize: number;
  total: number;
  onPageChange: (page: number) => void;
  onPageSizeChange: (pageSize: number) => void;
}) {
  const { t } = useTranslation('reports');
  if (total <= DEFAULT_REPORT_PAGE_SIZE) return null;

  return (
    <ResultPagination
      page={page}
      pageCount={pageCount}
      pageSize={pageSize}
      pageSizeOptions={REPORT_PAGE_SIZE_OPTIONS}
      pageLabel={t('pagination.page_summary', { page, pages: pageCount })}
      ariaLabel={t('pagination.aria')}
      pageSizeAriaLabel={t('pagination.page_size_aria')}
      firstAriaLabel={t('pagination.first_aria')}
      previousAriaLabel={t('pagination.previous_aria')}
      nextAriaLabel={t('pagination.next_aria')}
      lastAriaLabel={t('pagination.last_aria')}
      onPageChange={onPageChange}
      onPageSizeChange={onPageSizeChange}
      className="ml-auto w-fit gap-2 border-0 bg-transparent px-0"
    />
  );
}
