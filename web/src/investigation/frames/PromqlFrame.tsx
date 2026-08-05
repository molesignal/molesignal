import type { FrameProps } from '@/investigation/frame';

import { FramePlaceholder } from './_placeholder';

export function PromqlFrame(props: FrameProps) {
  return <FramePlaceholder {...props} label="PromQL" />;
}
