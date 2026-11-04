type StatusPageResultPage = {
  total: number;
  perPage: number;
};

export function shouldShowStatusPagePagination(
  result: StatusPageResultPage | null | undefined,
): result is StatusPageResultPage {
  return Boolean(result && result.perPage > 0 && result.total > result.perPage);
}
