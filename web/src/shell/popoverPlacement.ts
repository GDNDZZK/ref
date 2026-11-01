// Horizontal placement of a popover opened from a button inside a pane: the popover's right edge
// lines up with the button's right edge and it stays inside the pane. It narrows only when the
// pane is narrower than the popover.
export interface HorizontalSpan {
  left: number
  right: number
}

export interface PopoverPlacement {
  left: number
  width: number
}

export function placeInPane(
  button: HorizontalSpan,
  pane: HorizontalSpan,
  preferredWidth: number,
  margin: number,
): PopoverPlacement {
  const minLeft = pane.left + margin
  const maxRight = pane.right - margin
  const width = Math.max(0, Math.min(preferredWidth, maxRight - minLeft))
  const left = Math.min(Math.max(button.right - width, minLeft), maxRight - width)
  return { left, width }
}
