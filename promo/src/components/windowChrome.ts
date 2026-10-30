import { element } from '../engine/part.ts'

export const TITLE_BAR_HEIGHT_PX = 44

// A desktop-window look: title bar with three dots and a centred title, and a body below it.
export function createWindow(title: string): { frame: HTMLElement; body: HTMLElement } {
  const frame = element('div', 'part window')
  const bar = element('div', 'window-bar', frame)
  const dots = element('div', 'window-dots', bar)
  for (let dot = 0; dot < 3; dot++) element('span', 'window-dot', dots)
  const label = element('div', 'window-title', bar)
  label.textContent = title
  const body = element('div', 'window-body', frame)
  return { frame, body }
}
