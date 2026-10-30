// Limits of the conversation area. They come from the QML components, except the permission
// guard, which the GUI keeps in C++.
import { components } from '../theme/generated/components.ts'

export const PREVIEW_LINES = components.outputText.previewLines // output longer than this is cut behind "Show N more lines"
export const LIVE_TAIL_LINES = components.bashBody.liveTailLines // lines of live output shown while a command runs
export const THUMBNAIL_MAX_WIDTH = components.imageStrip.maxWidth
export const THUMBNAIL_MAX_HEIGHT = components.imageStrip.maxHeight
export const USER_THUMBNAIL_HEIGHT = components.imageBlock.thumbnailHeight
export const LIGHTBOX_MARGIN = components.lightbox.margin

export const OLDER_TRIGGER_DISTANCE = components.chatView.olderTriggerDistance // px from the top where older messages load by themselves
export const SHIMMER_PERIOD_MS = components.shimmerText.shimmerPeriod

export const PERMISSION_GUARD_MS = 350 // keys do not answer a request that just appeared
