import type { FrameProps } from '@/investigation/frame';

import { FramePlaceholder } from '../_placeholder';

export function LogRowFrame(props: FrameProps) {
  return <FramePlaceholder {...props} label="Log row" />;
}
