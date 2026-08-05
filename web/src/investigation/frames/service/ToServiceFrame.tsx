import type { FrameProps } from '@/investigation/frame';

import { FramePlaceholder } from '../_placeholder';

export function ServiceToServiceFrame(props: FrameProps) {
  return <FramePlaceholder {...props} label="Service ↔ Service" />;
}
