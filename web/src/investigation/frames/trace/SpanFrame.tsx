import type { FrameProps } from '@/investigation/frame';

import { FramePlaceholder } from '../_placeholder';

export function TraceSpanFrame(props: FrameProps) {
  return <FramePlaceholder {...props} label="Trace span" />;
}
