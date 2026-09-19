import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ResultPagination } from '@/shell/ResultPagination';

const PROFILE_PAGE_SIZE_OPTIONS = [10, 20, 50];
const DEFAULT_PROFILE_PAGE_SIZE = 10;

export function useProfilePagination<T>(items: readonly T[]) {
  const [page, setPage] = React.useState(1);
  const [pageSize, setPageSize] = React.useState(DEFAULT_PROFILE_PAGE_SIZE);
  const pageCount = Math.max(1, Math.ceil(items.length / pageSize));

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

export function ProfilePagination({
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
  const { t } = useTranslation('agent');
  if (total <= DEFAULT_PROFILE_PAGE_SIZE) return null;

  return (
    <ResultPagination
      page={page}
      pageCount={pageCount}
      pageSize={pageSize}
      pageSizeOptions={PROFILE_PAGE_SIZE_OPTIONS}
      pageLabel={t('settings.profiles.pagination.page_summary', {
        page,
        pages: pageCount,
      })}
      ariaLabel={t('settings.profiles.pagination.aria')}
      pageSizeAriaLabel={t('settings.profiles.pagination.page_size_aria')}
      firstAriaLabel={t('settings.profiles.pagination.first_aria')}
      previousAriaLabel={t('settings.profiles.pagination.previous_aria')}
      nextAriaLabel={t('settings.profiles.pagination.next_aria')}
      lastAriaLabel={t('settings.profiles.pagination.last_aria')}
      onPageChange={onPageChange}
      onPageSizeChange={onPageSizeChange}
      className="ml-auto w-fit gap-2 border-0 bg-transparent px-0"
    />
  );
}
