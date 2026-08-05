import * as React from 'react';

import type { FrameProps } from '@/investigation/frame';
import { useInvestigationStack } from '@/stores/useInvestigationStack';
import { LogStream } from '@/viz/log/LogStream';
import type { LogRow } from '@/viz/log/types';
import { useStreamingLogs } from '@/viz/log/useStreamingLogs';

export function LogFrame({ frame }: FrameProps) {
  const push = useInvestigationStack((s) => s.push);
  const [isLive, setIsLive] = React.useState(false);
  const params = frame.params as { stream?: string; statement?: string };
  const stream = params.stream;
  const statement = params.statement ?? (stream ? `SELECT * FROM ${stream} ORDER BY _timestamp DESC LIMIT 1000` : '');

  const { rows } = useStreamingLogs({
    url: '/api/v1/query/stream',
    body: { language: 'sql', statement, tail: isLive },
    enabled: !!statement,
  });

  const onRowOpen = (row: LogRow) => {
    push({ kind: 'log_row', params: { row }, parent_frame_id: frame.id });
  };

  return <LogStream rows={rows} isLive={isLive} onToggleLive={() => setIsLive((l) => !l)} onRowOpen={onRowOpen} />;
}
