// Guards hover-driven list selection against synthetic mousemove events fired when rows scroll
// underneath a stationary pointer during keyboard navigation.
export function createPointerGuard(): (event: MouseEvent) => boolean {
  let lastX = Number.NaN
  let lastY = Number.NaN
  return (event: MouseEvent): boolean => {
    if (event.clientX === lastX && event.clientY === lastY) return false
    lastX = event.clientX
    lastY = event.clientY
    return true
  }
}
