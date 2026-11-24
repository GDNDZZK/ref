import type { Message, MessageLayoutMeta, PartLayoutMeta } from '../proto/generated'
import { components } from '../theme/generated/components.ts'
import { metrics } from '../theme/generated/theme.ts'

export interface LayoutEstimateOptions {
  columnWidth: number
  thinkingExpanded: boolean
  detailsExpanded: boolean
  isFirst: boolean
}

const MIN_COLUMN_WIDTH = 200
const PROSE_CHAR_WIDTH = 8.2
const CODE_CHAR_WIDTH = 7.8
const PROSE_LINE_HEIGHT = 22
const CODE_LINE_HEIGHT = 19
const CODE_HEADER_HEIGHT = 34
const CODE_BOTTOM_PADDING = 12
const REASONING_HEADER_HEIGHT = 28
const REASONING_LINE_HEIGHT = 20
const REASONING_BOTTOM_PADDING = 6
const TOOL_HEADER_HEIGHT = 38
const TOOL_BODY_VERTICAL_PADDING = 20
const TOOL_LINE_HEIGHT = 19
const TOOL_TODO_ROW_HEIGHT = 26
const TOOL_DIFF_MAX_ROWS = 8
const TOOL_DIFF_ROW_HEIGHT = 20
const CAPTION_HEIGHT = 24
const TABLE_HEIGHT = 64
const COMPACTION_HEIGHT = 30
const ERROR_LINE_HEIGHT = 22
const META_TOP_GAP = 4
const MAX_TOOL_PREVIEW_LINES = components.outputText.previewLines
const BLOCK_SPACING = metrics.blockSpacing
const TURN_GAP = metrics.turnGap
const META_HEIGHT = metrics.metaLineHeight
const USER_V_PADDING = components.userBlock.verticalPadding * 2
const USER_H_PADDING = components.userBlock.horizontalPadding * 2
const USER_THUMB_HEIGHT = components.imageBlock.thumbnailHeight
const IMAGE_THUMB_HEIGHT = components.imageStrip.maxHeight
const ERROR_HEIGHT = components.errorBlock.verticalPadding * 2 + ERROR_LINE_HEIGHT
const TURN_OPENING_SOURCES = new Set(['human', 'peer', 'spawn'])

function wrappedLineCount(chars: number, lines: number, charsPerLine: number): number {
  const safeLines = Math.max(1, lines)
  const wrapped = chars > 0 ? Math.ceil(chars / Math.max(12, charsPerLine)) : 1
  return Math.max(safeLines, wrapped)
}

function estimateTextPartHeight(part: PartLayoutMeta, innerWidth: number): number {
  const proseCharsPerLine = Math.floor(innerWidth / PROSE_CHAR_WIDTH)
  const codeCharsPerLine = Math.floor((innerWidth - USER_H_PADDING) / CODE_CHAR_WIDTH)
  const proseChars = Math.max(0, part.chars - part.code_lines * 24)
  const proseLines = Math.max(0, part.lines - part.code_lines)
  const wrappedProse = proseChars > 0 || proseLines > 0 ? wrappedLineCount(proseChars, proseLines, proseCharsPerLine) : 0
  const wrappedCode =
    part.code_lines > 0 ? wrappedLineCount(part.code_lines * 36, part.code_lines, codeCharsPerLine) : 0
  const proseHeight = wrappedProse * PROSE_LINE_HEIGHT
  const codeOverhead = part.code_blocks * (CODE_HEADER_HEIGHT + CODE_BOTTOM_PADDING + BLOCK_SPACING)
  const codeHeight = wrappedCode * CODE_LINE_HEIGHT
  const tableHeight = part.tables * TABLE_HEIGHT
  return Math.max(PROSE_LINE_HEIGHT, proseHeight + codeOverhead + codeHeight + tableHeight)
}

function estimateToolPartHeight(part: PartLayoutMeta, detailsExpanded: boolean): number {
  if (part.tool === 'todo') {
    const todoRows = Math.max(1, part.tool_lines)
    return TOOL_HEADER_HEIGHT + 6 + todoRows * TOOL_TODO_ROW_HEIGHT
  }
  if (detailsExpanded) {
    const previewLines = Math.max(1, Math.min(part.tool_lines, MAX_TOOL_PREVIEW_LINES))
    return TOOL_HEADER_HEIGHT + TOOL_BODY_VERTICAL_PADDING + previewLines * TOOL_LINE_HEIGHT
  }
  if ((part.tool === 'edit' || part.tool === 'write') && part.tool_lines > 0) {
    const diffRows = Math.min(part.tool_lines, TOOL_DIFF_MAX_ROWS)
    return TOOL_HEADER_HEIGHT + TOOL_BODY_VERTICAL_PADDING + diffRows * TOOL_DIFF_ROW_HEIGHT + TOOL_DIFF_ROW_HEIGHT
  }
  return TOOL_HEADER_HEIGHT
}

function estimateImagePartHeight(part: PartLayoutMeta, innerWidth: number, maxHeight: number): number {
  if (part.width && part.height && part.width > 0 && part.height > 0) {
    const scale = Math.min(1, innerWidth / part.width, maxHeight / part.height)
    return Math.max(36, Math.round(part.height * scale))
  }
  return maxHeight
}

function estimateUserHeight(meta: MessageLayoutMeta, innerWidth: number, isFirst: boolean): number {
  const opensTurn = !isFirst && TURN_OPENING_SOURCES.has(meta.source_kind)
  const turnPrefix = opensTurn ? TURN_GAP * 2 + 1 : 0
  const caption = meta.source_kind !== 'human' ? CAPTION_HEIGHT : 0
  const bubbleInnerWidth = Math.max(120, innerWidth - USER_H_PADDING)
  const charsPerLine = Math.floor(bubbleInnerWidth / PROSE_CHAR_WIDTH)
  let textLines = 0
  let hasImage = false
  for (const part of meta.parts) {
    if (part.kind === 'image') {
      hasImage = true
      continue
    }
    textLines += wrappedLineCount(part.chars, part.lines, charsPerLine)
  }
  const bubbleHeight = USER_V_PADDING + caption + Math.max(1, textLines) * PROSE_LINE_HEIGHT
  const imageHeight = hasImage ? TURN_GAP + USER_THUMB_HEIGHT : 0
  return turnPrefix + bubbleHeight + imageHeight + META_TOP_GAP + META_HEIGHT
}

function estimateAssistantHeight(
  meta: MessageLayoutMeta,
  innerWidth: number,
  options: LayoutEstimateOptions,
): number {
  const partHeights: number[] = []
  for (const part of meta.parts) {
    if (part.kind === 'text') {
      partHeights.push(estimateTextPartHeight(part, innerWidth))
    } else if (part.kind === 'reasoning') {
      const body = options.thinkingExpanded
        ? REASONING_BOTTOM_PADDING +
          wrappedLineCount(part.chars, part.lines, Math.floor(innerWidth / PROSE_CHAR_WIDTH)) *
            REASONING_LINE_HEIGHT
        : 0
      partHeights.push(REASONING_HEADER_HEIGHT + body)
    } else if (part.kind === 'tool') {
      partHeights.push(estimateToolPartHeight(part, options.detailsExpanded))
    } else if (part.kind === 'image') {
      partHeights.push(estimateImagePartHeight(part, innerWidth, IMAGE_THUMB_HEIGHT))
    }
  }
  let total = partHeights.reduce((sum, height) => sum + height, 0)
  if (partHeights.length > 1) total += (partHeights.length - 1) * BLOCK_SPACING
  if (meta.has_error) total += (partHeights.length > 0 ? BLOCK_SPACING : 0) + ERROR_HEIGHT
  return total + (total > 0 ? META_TOP_GAP : 0) + META_HEIGHT
}

export function estimateMessageHeight(meta: MessageLayoutMeta, options: LayoutEstimateOptions): number {
  if (meta.source_kind === 'compaction') return COMPACTION_HEIGHT
  const innerWidth = Math.max(MIN_COLUMN_WIDTH, Math.min(metrics.contentWidth, options.columnWidth))
  if (meta.role === 'user') return estimateUserHeight(meta, innerWidth, options.isFirst)
  return estimateAssistantHeight(meta, innerWidth, options)
}

function countLines(text: string): number {
  if (text === '') return 0
  let count = 1
  for (let i = 0; i < text.length; i++) {
    if (text.charCodeAt(i) === 10) count++
  }
  return count
}

function analyzeMarkdown(text: string): { codeBlocks: number; codeLines: number; tables: number } {
  let codeBlocks = 0
  let codeLines = 0
  let tables = 0
  let inFence = false
  let prevHasPipe = false
  for (const rawLine of text.split('\n')) {
    const line = rawLine.trimStart()
    if (line.startsWith('```') || line.startsWith('~~~')) {
      if (!inFence) codeBlocks++
      inFence = !inFence
      prevHasPipe = false
      continue
    }
    if (inFence) {
      codeLines++
      continue
    }
    const hasPipe = line.includes('|')
    if (hasPipe && prevHasPipe && /^[\s|:-]+$/.test(line)) {
      tables++
    }
    prevHasPipe = hasPipe
  }
  return { codeBlocks, codeLines, tables }
}

function toolMetaLines(part: Extract<Message['parts'][number], { type: 'tool' }>): number {
  if (part.name === 'todo') {
    const input = part.input as { todos?: unknown } | null
    return Array.isArray(input?.todos) ? input.todos.length : 0
  }
  const meta = part.meta as { diff?: unknown } | null
  if (typeof meta?.diff === 'string') return countLines(meta.diff)
  return part.output ? countLines(part.output) : 0
}

export function layoutMetaFromMessage(seq: number, message: Message): MessageLayoutMeta {
  const parts: PartLayoutMeta[] = message.parts.map((part) => {
    if (part.type === 'text') {
      const md = analyzeMarkdown(part.text)
      return {
        kind: 'text',
        chars: part.text.length,
        lines: countLines(part.text),
        code_blocks: md.codeBlocks,
        code_lines: md.codeLines,
        tables: md.tables,
        tool: null,
        tool_lines: 0,
        width: null,
        height: null,
      }
    }
    if (part.type === 'reasoning') {
      return {
        kind: 'reasoning',
        chars: part.text.length,
        lines: countLines(part.text),
        code_blocks: 0,
        code_lines: 0,
        tables: 0,
        tool: null,
        tool_lines: 0,
        width: null,
        height: null,
      }
    }
    if (part.type === 'image') {
      return {
        kind: 'image',
        chars: 0,
        lines: 0,
        code_blocks: 0,
        code_lines: 0,
        tables: 0,
        tool: null,
        tool_lines: 0,
        width: null,
        height: null,
      }
    }
    const toolLines = toolMetaLines(part)
    return {
      kind: 'tool',
      chars: part.purpose.length + (part.output?.length ?? 0),
      lines: toolLines,
      code_blocks: 0,
      code_lines: 0,
      tables: 0,
      tool: part.name,
      tool_lines: toolLines,
      width: null,
      height: null,
    }
  })

  return {
    seq,
    id: message.id,
    role: message.role,
    source_kind: message.source.kind,
    has_error: message.error !== null,
    parts,
  }
}
