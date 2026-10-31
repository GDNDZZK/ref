import { lerp, span } from '../engine/anim.ts'
import { tr, type Localized } from '../engine/i18n.ts'
import { element, type Part } from '../engine/part.ts'
import { FRAME_WIDTH } from '../engine/player.ts'

export interface CompareRow {
  label: string | Localized
  // One cell per column, in column order; the first column is ref and is outlined.
  cells: readonly (string | Localized)[]
}

export interface CompareCardOptions {
  title: string | Localized
  columns: readonly string[]
  rows: readonly CompareRow[]
  footer: string | Localized
  // The header shows alone this long, then each row gets `rowSeconds`.
  headerSeconds: number
  rowSeconds: number
  from: number
  to: number
}

const CARD_TOP_PX = 70
const CARD_WIDTH_PX = 1720
const ROW_ZOOM = 1.1
// Stage height at which the active row is centred.
const ROW_TARGET_Y_PX = 560
const MOVE_SECONDS = 0.6
const ROW_ENTER_SECONDS = 0.5
const ROW_RISE_PX = 24
const ENTER_SECONDS = 0.6
const EXIT_SECONDS = 0.3

interface View {
  zoom: number
  x: number
  y: number
}

function mix(from: View, to: View, progress: number): View {
  return { zoom: lerp(from.zoom, to.zoom, progress), x: lerp(from.x, to.x, progress), y: lerp(from.y, to.y, progress) }
}

// A table of what each project documents, one row at a time; the camera moves to the active row.
// Plain text cells in one type size, no ticks or crosses, the ref column outlined.
export function compareCard(options: CompareCardOptions): Part {
  const el = element('div', 'part compare-stage')
  const card = element('div', 'compare-card', el)
  Object.assign(card.style, { left: `${(FRAME_WIDTH - CARD_WIDTH_PX) / 2}px`, top: `${CARD_TOP_PX}px`, width: `${CARD_WIDTH_PX}px` })
  element('h2', 'compare-title', card).textContent = tr(options.title)
  const table = element('div', 'compare-table', card)
  table.style.gridTemplateColumns = `1.15fr repeat(${options.columns.length}, 1fr)`
  element('div', 'compare-head', table)
  options.columns.forEach((column, index) => {
    element('div', `compare-head${index === 0 ? ' is-ref' : ''}`, table).textContent = column
  })
  const rows = options.rows.map((row) => {
    const cells = [element('div', 'compare-label', table)]
    cells[0]!.textContent = tr(row.label)
    row.cells.forEach((text, index) => {
      const cell = element('div', `compare-cell${index === 0 ? ' is-ref' : ''}`, table)
      cell.textContent = tr(text)
      cells.push(cell)
    })
    return cells
  })
  element('div', 'compare-footer', card).textContent = tr(options.footer)

  const rowStart = (index: number) => options.from + options.headerSeconds + index * options.rowSeconds
  const overview: View = { zoom: 1, x: 0, y: 0 }
  // The view that centres row `index`, measured from the laid-out card (the cells' offset parent).
  const rowView = (index: number): View => {
    const cells = rows[index]!
    const top = Math.min(...cells.map((cell) => cell.offsetTop))
    const bottom = Math.max(...cells.map((cell) => cell.offsetTop + cell.offsetHeight))
    const centre = (top + bottom) / 2
    return { zoom: ROW_ZOOM, x: (CARD_WIDTH_PX * (1 - ROW_ZOOM)) / 2, y: ROW_TARGET_Y_PX - CARD_TOP_PX - ROW_ZOOM * centre }
  }

  return {
    el,
    render(time) {
      let view = overview
      rows.forEach((cells, index) => {
        const start = rowStart(index)
        view = mix(view, rowView(index), span(time, start, MOVE_SECONDS, 'inOutSine'))
        const shown = span(time, start, ROW_ENTER_SECONDS, 'outExpo')
        for (const cell of cells) {
          cell.style.opacity = String(shown)
          cell.style.transform = `translateY(${(1 - shown) * ROW_RISE_PX}px)`
        }
      })
      card.style.transform = `translate(${view.x}px, ${view.y}px) scale(${view.zoom})`
      const entering = span(time, options.from, ENTER_SECONDS, 'outExpo')
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      el.style.opacity = String(entering * (1 - leaving))
    },
  }
}
