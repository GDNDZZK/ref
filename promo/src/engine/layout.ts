import { FRAME_HEIGHT, FRAME_WIDTH } from './player.ts'

// Where a captured window sits on the stage: centred, with room above for the feature tag
// and below for the caption.
export const WINDOW_WIDTH_PX = 1280
export const WINDOW_TOP_PX = 88
export const CAPTION_BAND_PX = 130

export function windowRect(picture: { width: number; height: number }, titleBarPx: number) {
  const pictureHeight = (WINDOW_WIDTH_PX * picture.height) / picture.width
  return {
    x: (FRAME_WIDTH - WINDOW_WIDTH_PX) / 2,
    y: Math.min(WINDOW_TOP_PX, FRAME_HEIGHT - CAPTION_BAND_PX - pictureHeight - titleBarPx),
    width: WINDOW_WIDTH_PX,
    height: pictureHeight + titleBarPx,
  }
}
