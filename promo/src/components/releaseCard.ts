import { span } from '../engine/anim.ts'
import { tr, type Localized } from '../engine/i18n.ts'
import { element, type Part } from '../engine/part.ts'

export interface ReleaseRow {
  system: string | Localized
  archive: string
}

export interface ReleaseCardOptions {
  title: string | Localized
  rows: readonly ReleaseRow[]
  // Index of the row that lights up, and when.
  highlight: number
  highlightAt: number
  from: number
  to: number
}

const ROW_STAGGER_SECONDS = 0.08
const ROW_ENTER_SECONDS = 0.5
const HIGHLIGHT_SECONDS = 0.4
const ENTER_SECONDS = 0.6
const EXIT_SECONDS = 0.3

// A release page drawn as a list of archives, one row per system; one row is picked.
export function releaseCard(options: ReleaseCardOptions): Part {
  const el = element('div', 'part release-card')
  element('div', 'release-title', el).textContent = tr(options.title)
  const rows = options.rows.map((row) => {
    const rowEl = element('div', 'release-row', el)
    element('span', 'release-system', rowEl).textContent = tr(row.system)
    element('span', 'release-archive', rowEl).textContent = row.archive
    return rowEl
  })

  return {
    el,
    render(time) {
      rows.forEach((row, index) => {
        const shown = span(time, options.from + 0.2 + index * ROW_STAGGER_SECONDS, ROW_ENTER_SECONDS, 'outExpo')
        row.style.opacity = String(shown)
        const lit = index === options.highlight ? span(time, options.highlightAt, HIGHLIGHT_SECONDS, 'inOutSine') : 0
        row.style.setProperty('--lit', String(lit))
      })
      const entering = span(time, options.from, ENTER_SECONDS, 'outExpo')
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      el.style.opacity = String(entering * (1 - leaving))
    },
  }
}
