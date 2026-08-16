type StatusPageResultPage = {
  total: number;
  per_page: number;
};

export function shouldShowStatusPagePagination(
  result: StatusPageResultPage | null | undefined,
): result is StatusPageResultPage {
  return Boolean(result && result.per_page > 0 && result.total > result.per_page);
}
