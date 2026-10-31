// C7 "Bring your opencode.json" (shooting script section 5), real backend with a fixture home (RB1).
import { textCard } from '../components/textCard.ts'
import { fakeTerminal } from '../components/fakeTerminal.ts'
import { castWindow } from '../shots/castWindow.ts'
import { HOOK_RECT, terminalCut } from '../shots/terminalHook.ts'
import { FEATURES } from './features.ts'
import { cues, drawnCut, endCut, timelineFilm } from './timeline.ts'

const HOOK_END = 3.0
const CONFIG_END = 7.0
const DIALOGS_END = 17.0
const KEYS_END = 20.0
const FILM_END = 23.5

// A config of some agent tool, no brand: providers, agents, permissions.
const SCROLLED_CONFIG = [
  '{',
  '  "providers": {',
  '    "anthropic": { "models": ["claude-sonnet-4-5"] },',
  '    "deepseek": { "models": ["deepseek-chat"] },',
  '    "local": {',
  '      "baseURL": "http://localhost:8080/v1",',
  '      "models": ["qwen3-coder"]',
  '    }',
  '  },',
  '  "agents": {',
  '    "review": {',
  '      "model": "anthropic/claude-sonnet-4-5",',
  '      "prompt": "Review the diff. Point out bugs first."',
  '    },',
  '    "docs": { "model": "deepseek/deepseek-chat" }',
  '  },',
  '  "permissions": {',
  '    "bash": { "*": "ask", "git status*": "allow" },',
  '    "edit": "ask"',
  '  },',
  '  "commands": {',
  '    "test": "cargo test --workspace",',
  '    "lint": "cargo clippy -- -D warnings"',
  '  },',
  '  "theme": "dark",',
  '  "keybinds": { "model_list": "ctrl+x m" }',
  '}',
]
const SCROLL_ROWS_PER_SECOND = 4.5

// The file ref reads, as the viewer has it.
const OPENCODE_JSON = [
  '{',
  '  "model": "anthropic/claude-sonnet-4-5",',
  '  "provider": {',
  '    "local": {',
  '      "npm": "@ai-sdk/openai-compatible",',
  '      "options": {',
  '        "baseURL": "http://localhost:8080/v1",',
  '        "apiKey": "{env:WORK_API_KEY}"',
  '      },',
  '      "models": { "qwen3-coder": {} }',
  '    }',
  '  },',
  '  "agent": { "review": { "description": "Reviews the diff" } },',
  '  "permission": { "bash": "ask" }',
  '}',
]
const API_KEY_LINE = OPENCODE_JSON.findIndex((line) => line.includes('apiKey'))
const CARD = { x: 360, y: 90, width: 1200 }

// Cast seconds of tui-fixture-start where shot 3 begins (`cd ~/project` is typed); the model
// and agent dialogs follow in the same take.
const CAST_START_SECONDS = 0

export const scene = timelineFilm({
  id: 'opencode',
  cuts: [
    terminalCut(0, HOOK_END, {
      lines: SCROLLED_CONFIG.map((text) => ({ at: -1, text })),
      scroll: (time) => Math.max(0, time) * SCROLL_ROWS_PER_SECOND,
    }),
    drawnCut(HOOK_END, CONFIG_END, (shot, seconds) =>
      shot.add(
        textCard({
          ...CARD,
          title: '~/.config/opencode/opencode.json',
          lines: OPENCODE_JSON.map((text) => ({ text })),
          from: 0,
          to: seconds + 1,
        }),
      ),
    ),
    drawnCut(CONFIG_END, DIALOGS_END, (shot, seconds) =>
      shot.add(castWindow({ name: 'tui-fixture-start', title: 'ref', x: 200, y: 96, fontSize: 22, start: -CAST_START_SECONDS, from: -1, to: seconds + 1 })),
    ),
    drawnCut(DIALOGS_END, KEYS_END, (shot, seconds) => {
      shot.add(
        textCard({
          ...CARD,
          title: '~/.config/opencode/opencode.json',
          lines: OPENCODE_JSON.map((text, index) => ({ text, tone: index === API_KEY_LINE ? 'good' : 'plain' })),
          from: 0,
          to: 1.6,
        }),
      )
      shot.add(
        fakeTerminal({
          rect: HOOK_RECT,
          lines: [{ at: 1.7, typed: true, prompt: '$ ', text: 'export DEEPSEEK_API_KEY=...', charsPerSecond: 24 }],
          from: 1.4,
          to: seconds + 1,
        }),
      )
    }),
    endCut(KEYS_END, FILM_END, {
      note: { en: 'Some opencode keys are ignored. See docs/configuration.md.', zh: '部分 opencode 配置项会被忽略，见 docs/configuration.md。' },
    }),
  ],
  captions: cues([
    [0.2, 2.9, { en: 'Switching tools means redoing your config.', zh: '换工具，配置又得重写一遍。' }],
    [3.2, 6.8, { en: 'ref reads your opencode.json.', zh: 'ref 直接读你的 opencode.json。' }],
    [7.2, 10.8, { en: 'Run ref in your project.', zh: '在项目里运行 ref。' }],
    [11.2, 14.0, { en: 'Your models are in the model list.', zh: '你配置的模型都在列表里。' }],
    [14.3, 16.8, { en: 'Your agents and commands load too.', zh: '你的智能体和命令也会加载。' }],
    [17.2, 19.9, { en: 'Keys come from environment variables.', zh: '密钥从环境变量里读。' }],
  ]),
  chip: { ...FEATURES.opencode, from: HOOK_END + 0.1, to: KEYS_END },
})
