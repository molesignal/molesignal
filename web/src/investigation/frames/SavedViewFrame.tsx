import type { FrameProps } from '@/investigation/frame';

import { FramePlaceholder } from './_placeholder';

export function SavedViewFrame(props: FrameProps) {
  return <FramePlaceholder {...props} label="Saved view" />;
}
