import { FEATURES } from './features.ts'
import { cardsBeat, filmScene, tagOf, windowBeat } from './film.ts'

const feature = FEATURES.setup
const tag = tagOf(feature)

// docs/configuration.md, "A complete example" (shortened).
const CONFIG_LINES = [
  '{',
  '  "model": "anthropic/claude-sonnet-4-5",',
  '  "agent": { "build": { "variant": "high" } },',
  '  "mcp": {',
  '    "github": {',
  '      "type": "local",',
  '      "command": ["npx", "-y", "@modelcontextprotocol/server-github"]',
  '    }',
  '  }',
  '}',
]
const CONFIG_TYPE_START = 0.6
const CONFIG_LINE_SECONDS = 0.65

export const scene = filmScene(feature, [
  cardsBeat({
    seconds: 8,
    tag,
    cards: [
      {
        x: 160,
        y: 170,
        width: 1600,
        title: 'opencode.json',
        lines: CONFIG_LINES.map((text, index) => ({
          text,
          typeAt: CONFIG_TYPE_START + index * CONFIG_LINE_SECONDS,
          charsPerSecond: 70,
        })),
      },
    ],
    caption: { en: 'Bring your opencode.json.', zh: '带上你的 opencode.json。' },
  }),
  windowBeat({
    seconds: 5,
    tag,
    screens: [{ name: 'gui-models-dialog' }],
    looks: [
      { at: 0, x: 0.5, y: 0.45, zoom: 1 },
      { at: 1, x: 0.5, y: 0.38, zoom: 2.2 },
    ],
    caption: { en: 'Any model provider. Switch per session.', zh: '任意模型提供商，按会话切换。' },
  }),
  windowBeat({
    seconds: 5,
    tag,
    screens: [{ name: 'gui-agents-dialog' }],
    looks: [
      { at: 0, x: 0.5, y: 0.45, zoom: 1 },
      { at: 1, x: 0.5, y: 0.4, zoom: 2 },
    ],
    caption: { en: 'Agents from your config, each with its own rules.', zh: '配置里的智能体，各有各的规则。' },
  }),
])
