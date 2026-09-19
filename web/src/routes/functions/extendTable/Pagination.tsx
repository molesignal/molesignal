import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ResultPagination } from '@/shell/ResultPagination';

const EXTEND_TABLE_PAGE_SIZE_OPTIONS = [10, 20, 50];
const DEFAULT_EXTEND_TABLE_PAGE_SIZE = 10;

export function useExtendTablePagination<T>(
  items: readonly T[],
  resetKey: string,
) {
  const [page, setPage] = React.useState(1);
  const [pageSize, setPageSize] = React.useState(
    DEFAULT_EXTEND_TABLE_PAGE_SIZE,
  );
  const pageCount = Math.max(1, Math.ceil(items.length / pageSize));
  const currentPage = Math.min(page, pageCount);

  React.useEffect(() => {
    setPage(1);
  }, [resetKey]);

  React.useEffect(() => {
    setPage((current) => Math.min(current, pageCount));
  }, [pageCount]);

  const pageItems = React.useMemo(() => {
    const start = (currentPage - 1) * pageSize;
    return items.slice(start, start + pageSize);
  }, [currentPage, items, pageSize]);

  const changePageSize = React.useCallback((nextPageSize: number) => {
    setPageSize(nextPageSize);
    setPage(1);
  }, []);

  return {
    page: currentPage,
    pageCount,
    pageSize,
    pageItems,
    total: items.length,
    onPageChange: setPage,
    onPageSizeChange: changePageSize,
  };
}

export function ExtendTablePagination({
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
  const { t } = useTranslation('functions');
  if (total <= DEFAULT_EXTEND_TABLE_PAGE_SIZE) return null;

  return (
    <ResultPagination
      page={page}
      pageCount={pageCount}
      pageSize={pageSize}
      pageSizeOptions={EXTEND_TABLE_PAGE_SIZE_OPTIONS}
      pageLabel={t('extend_tables.pagination.page_summary', {
        page,
        pages: pageCount,
      })}
      ariaLabel={t('extend_tables.pagination.aria')}
      pageSizeAriaLabel={t('extend_tables.pagination.page_size_aria')}
      firstAriaLabel={t('extend_tables.pagination.first_aria')}
      previousAriaLabel={t('extend_tables.pagination.previous_aria')}
      nextAriaLabel={t('extend_tables.pagination.next_aria')}
      lastAriaLabel={t('extend_tables.pagination.last_aria')}
      onPageChange={onPageChange}
      onPageSizeChange={onPageSizeChange}
      className="border-t border-bd-0 bg-transparent px-4"
    />
  );
}
