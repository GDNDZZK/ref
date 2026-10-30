// Limits of the conversation area. Most come from the QML components; the last three have no
// property in the GUI (it uses its own mechanisms there).
import { components } from '../theme/generated/components.ts'

export const PREVIEW_LINES = components.outputText.previewLines // output longer than this is cut behind "Show N more lines"
export const LIVE_TAIL_LINES = components.bashBody.liveTailLines // lines of live output shown while a command runs
export const THUMBNAIL_MAX_WIDTH = components.imageStrip.maxWidth
export const THUMBNAIL_MAX_HEIGHT = components.imageStrip.maxHeight
export const USER_THUMBNAIL_HEIGHT = components.imageBlock.thumbnailHeight
export const LIGHTBOX_MARGIN = components.lightbox.margin

export const PERMISSION_GUARD_MS = 350 // keys do not answer a request that just appeared
export const OLDER_TRIGGER_DISTANCE = 400 // px from the top where older messages load by themselves
export const SHIMMER_PERIOD_MS = 1600
