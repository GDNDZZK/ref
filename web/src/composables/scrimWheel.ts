// Prevents wheel events on a modal scrim from scrolling the conversation behind it unless
// an ancestor inside the scrim can still scroll in the wheel's vertical direction.
const SCROLL_END_TOLERANCE = 1

function isScrollableY(element: HTMLElement): boolean {
  const { overflowY } = getComputedStyle(element)
  return overflowY === 'auto' || overflowY === 'scroll'
}

function canScrollY(element: HTMLElement, deltaY: number): boolean {
  if (!isScrollableY(element) || element.scrollHeight <= element.clientHeight) return false
  if (deltaY < 0) return element.scrollTop > 0
  if (deltaY > 0)
    return element.scrollHeight - element.clientHeight - element.scrollTop >= SCROLL_END_TOLERANCE
  return false
}

export function containScrimWheel(event: WheelEvent): void {
  if (event.ctrlKey) return
  const scrim = event.currentTarget
  if (!(scrim instanceof HTMLElement)) return
  if (event.deltaY === 0) {
    if (event.deltaX !== 0) event.preventDefault()
    return
  }

  let node: Element | null = event.target instanceof Element ? event.target : null
  while (node && node !== scrim) {
    if (node instanceof HTMLElement && canScrollY(node, event.deltaY)) return
    node = node.parentElement
  }
  event.preventDefault()
}
