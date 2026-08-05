import { HelpCircle } from 'lucide-react';
import type { ReactNode } from 'react';

import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';

type QuerySyntaxMode = 'fields' | 'sql' | 'promql';
type QuerySyntaxScope = 'logs' | 'traces' | 'metrics';
type FieldSqlScope = Exclude<QuerySyntaxScope, 'metrics'>;

interface QuerySyntaxHelpProps {
  mode: QuerySyntaxMode;
  scope: QuerySyntaxScope;
  className?: string | undefined;
  contentClassName?: string | undefined;
  ariaLabel?: string | undefined;
  triggerTitle?: string | undefined;
  title?: string | undefined;
  description?: string | undefined;
  examples?: SyntaxExample[] | undefined;
  footer?: ReactNode;
}

interface SyntaxExample {
  label: string;
  expression: string;
  description: string;
}

const FIELD_EXAMPLES: Record<FieldSqlScope, SyntaxExample[]> = {
  logs: [
    { label: '全文搜索', expression: 'error', description: '匹配 message 文本。' },
    { label: '字段等值', expression: "service = 'checkout'", description: '只返回指定字段值。' },
    { label: '字段包含', expression: "message contains 'timeout'", description: '字段内包含关键字。' },
    { label: '排除条件', expression: "level != 'debug'", description: '排除指定字段值。' },
    { label: '多条件', expression: "service = 'checkout' AND level = 'error'", description: '使用 AND 组合条件。' },
  ],
  traces: [
    { label: 'Trace ID', expression: "trace_id = '4bf92f3577b34da6a3ce929d0e0e4736'", description: '定位单条 trace。' },
    { label: '服务名', expression: "service_name contains 'checkout'", description: '按服务过滤。' },
    { label: '操作名', expression: "operation_name contains 'GET /api'", description: '按 span 操作过滤。' },
    { label: '错误状态', expression: "status_code = 'ERROR'", description: '只看异常 span。' },
    { label: '多条件', expression: "service_name = 'checkout' AND status_code = 'ERROR'", description: '使用 AND 组合条件。' },
  ],
};

const SQL_EXAMPLES: Record<FieldSqlScope, SyntaxExample[]> = {
  logs: [
    { label: '基础查询', expression: `SELECT * FROM "app_logs" ORDER BY _timestamp DESC LIMIT 200`, description: '按时间倒序读取日志。' },
    { label: '字段过滤', expression: `WHERE "level" = 'error'`, description: '字段精确匹配。' },
    { label: '模糊匹配', expression: `WHERE "message" LIKE '%timeout%'`, description: '字段文本包含关键字。' },
    { label: '数值过滤', expression: `WHERE "status_code" >= 500`, description: '用于状态码、耗时等数值字段。' },
  ],
  traces: [
    { label: '基础查询', expression: `SELECT trace_id, COUNT(*) AS span_count FROM traces GROUP BY trace_id`, description: '按 trace 汇总 span。' },
    { label: '服务过滤', expression: `WHERE "service_name" = 'checkout'`, description: '限制服务范围。' },
    { label: '错误过滤', expression: `WHERE "status_code" = 'ERROR'`, description: '只看错误 span。' },
    { label: '耗时排序', expression: `ORDER BY duration_ms DESC LIMIT 50`, description: '查看慢 trace 或慢 span。' },
  ],
};

function defaultExamples(mode: QuerySyntaxMode, scope: QuerySyntaxScope): SyntaxExample[] {
  if (scope === 'metrics') return [];
  return mode === 'sql' ? SQL_EXAMPLES[scope] : FIELD_EXAMPLES[scope];
}

export function QuerySyntaxHelp({
  mode,
  scope,
  className,
  contentClassName,
  ariaLabel = '打开查询语法提示',
  triggerTitle = '查询语法提示',
  title = '语法提示',
  description,
  examples: suppliedExamples,
  footer,
}: QuerySyntaxHelpProps) {
  const examples = suppliedExamples ?? defaultExamples(mode, scope);
  const modeLabel = mode === 'sql' ? 'SQL' : mode === 'promql' ? 'PromQL' : 'Fields';
  const resolvedDescription = description
    ?? `当前为 ${modeLabel} 查询模式，按 Cmd/Ctrl + Enter 执行查询。`;

  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="outline"
          size="icon"
          className={cn(
            'h-[26px] w-[26px] rounded-md border-bd-1 bg-bg-2 text-tx-2 hover:bg-bg-3 hover:text-tx-0',
            className,
          )}
          aria-label={ariaLabel}
          title={triggerTitle}
        >
          <HelpCircle className="h-3.5 w-3.5" />
        </Button>
      </PopoverTrigger>
      <PopoverContent
        side="bottom"
        align="start"
        className={cn(
          'w-[560px] overflow-hidden rounded-md border-bd-1 bg-bg-0 p-0 text-tx-1 shadow-drawer',
          contentClassName,
        )}
      >
        <div className="border-b border-bd-0 bg-bg-1 px-4 py-3">
          <div className="font-sans text-sm font-bold text-tx-0">{title}</div>
          <div className="mt-1 font-sans text-xs text-tx-3">
            {resolvedDescription}
          </div>
        </div>
        <div className="space-y-2.5 px-4 py-3 font-sans text-xs">
          {examples.map((example) => (
            <div key={`${example.label}-${example.expression}`} className="grid grid-cols-[78px_minmax(0,1fr)] gap-3">
              <span className="pt-0.5 font-semibold text-tx-2">{example.label}</span>
              <div className="min-w-0">
                <code className="inline-block max-w-full overflow-x-auto rounded border border-bd-0 bg-bg-2 px-1.5 py-0.5 font-mono text-xs text-tx-0">
                  {example.expression}
                </code>
                <div className="mt-1 text-xs text-tx-3">{example.description}</div>
              </div>
            </div>
          ))}
        </div>
        {footer !== null ? (
          <div className="border-t border-bd-0 bg-bg-1 px-4 py-2 font-sans text-xs text-tx-3">
            {footer ?? '左侧字段列表的加号会把字段条件添加到当前查询；同一字段不会重复添加。'}
          </div>
        ) : null}
      </PopoverContent>
    </Popover>
  );
}
