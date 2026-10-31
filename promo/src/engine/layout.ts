import { FRAME_HEIGHT, FRAME_WIDTH } from './player.ts'

// Where a captured window sits on the stage: centred, with room above for the feature tag
// and below for the caption.
export const WINDOW_WIDTH_PX = 1280
export const WINDOW_TOP_PX = 88
export const CAPTION_BAND_PX = 130

// A window as wide as WINDOW_WIDTH_PX. A picture too tall for the stage (a tall GUI window) is
// scaled down to fit instead.
export function windowRect(picture: { width: number; height: number }, titleBarPx: number) {
  const availableHeight = FRAME_HEIGHT - CAPTION_BAND_PX - WINDOW_TOP_PX - titleBarPx
  const fullHeight = (WINDOW_WIDTH_PX * picture.height) / picture.width
  const scale = Math.min(1, availableHeight / fullHeight)
  const width = WINDOW_WIDTH_PX * scale
  const pictureHeight = fullHeight * scale
  return {
    x: (FRAME_WIDTH - width) / 2,
    y: Math.min(WINDOW_TOP_PX, FRAME_HEIGHT - CAPTION_BAND_PX - pictureHeight - titleBarPx),
    width,
    height: pictureHeight + titleBarPx,
  }
}
