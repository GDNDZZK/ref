// Terminal look shared by the video scenes (terminalView) and the README stills (capture/tui.ts).
export const TERMINAL_FONT = "'DejaVu Sans Mono', ui-monospace, Menlo, Consolas, monospace"

const ANSI_COLOR_NAMES = ['black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white'] as const
const ANSI_COLORS = [
  '#1b1c20', '#ec6f73', '#6cc28a', '#e0b86a', '#7aa2f7', '#bb9af7', '#7dcfff', '#c8c9d0',
  '#4a4d56', '#f28b8f', '#8ad9a3', '#efca85', '#93b5fa', '#cdb2fa', '#9adcff', '#e6e6e9',
]

function ansiColors(): Record<string, string> {
  const colors: Record<string, string> = {}
  ANSI_COLORS.forEach((color, index) => {
    const name = ANSI_COLOR_NAMES[index % ANSI_COLOR_NAMES.length]!
    const key = index < ANSI_COLOR_NAMES.length ? name : `bright${name[0]!.toUpperCase()}${name.slice(1)}`
    colors[key] = color
  })
  return colors
}

// Product dark theme: --background, --text and --selection of the generated theme.
export const TERMINAL_THEME: Record<string, string> = {
  background: '#16171a',
  foreground: '#e6e6e9',
  cursor: '#e6e6e9',
  selectionBackground: '#2f4a7a',
  ...ansiColors(),
}
