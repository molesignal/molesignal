import { useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';

import * as queryApi from '@/api/query';
import type { CodeCompletionItem } from '@/shell/codeEditor/types';

const FALLBACK_SQL_FUNCTIONS: ReadonlyArray<queryApi.SqlFunctionCapabilityItem> = [
  {
    label: 'MATCH',
    insert_text: "MATCH(${1:field}, '${2:term}')",
    detail: '任意字段子串匹配，大小写不敏感',
    documentation: '无索引前提。term 中的 % / _ 按字面量处理；空 term 恒不匹配。',
    kind: 'function',
  },
  {
    label: 'MATCH_TEXT',
    insert_text: "MATCH_TEXT(${1:field}, '${2:query}')",
    detail: '全文检索（多词 / 短语 / 通配符）',
    documentation: '仅限已配置 full_text 索引的 string 字段（indexed && !exact），未配置时报错。',
    kind: 'function',
  },
];

/**
 * SQL 文本检索函数（`MATCH` / `MATCH_TEXT`）补全项，由后端 `/query/sql/capabilities`
 * 驱动——引擎支持哪些函数，前端就提示哪些（与 PromQL capabilities 同模式）。
 * 能力接口尚未加载或旧后端没有该接口时，用当前引擎支持的函数兜底，避免 Fields
 * 模式只剩字段和操作符提示。接口成功返回后仍以服务端清单为准。
 */
export function useSqlFunctionCompletions(): CodeCompletionItem[] {
  const { data } = useQuery({
    queryKey: ['sql-query-capabilities'],
    queryFn: queryApi.fetchSqlQueryCapabilities,
    staleTime: 5 * 60 * 1000,
  });
  return useMemo(
    () => resolveSqlFunctionCompletions(data),
    [data],
  );
}

export function resolveSqlFunctionCompletions(
  capabilities: queryApi.SqlQueryCapabilities | undefined,
): CodeCompletionItem[] {
  const functions = capabilities?.functions ?? FALLBACK_SQL_FUNCTIONS;
  return functions.map((item) => {
    const label = item.label.trim().toUpperCase();
    return {
      label,
      insertText: uppercaseSnippetFunctionName(item.insert_text, label),
      insertTextFormat: 'snippet',
      kind: 'function',
      detail: item.detail,
      documentation: item.documentation,
    };
  });
}

function uppercaseSnippetFunctionName(insertText: string, label: string): string {
  return insertText.replace(
    /^(\s*)[a-z_][\w$]*/i,
    (_match, leadingWhitespace: string) => `${leadingWhitespace}${label}`,
  );
}
