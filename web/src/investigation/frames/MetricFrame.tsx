import type { FrameProps } from '@/investigation/frame';

import { FramePlaceholder } from './_placeholder';

export function MetricFrame(props: FrameProps) {
  return <FramePlaceholder {...props} label="Metric" />;
}
