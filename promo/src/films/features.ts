import clockIcon from '../../../assets/icons/clock-16.svg?raw'
import lockIcon from '../../../assets/icons/lock-16.svg?raw'
import pencilIcon from '../../../assets/icons/pencil-16.svg?raw'
import questionIcon from '../../../assets/icons/question-16.svg?raw'
import retryIcon from '../../../assets/icons/retry-16.svg?raw'
import slidersIcon from '../../../assets/icons/sliders-20.svg?raw'
import splitIcon from '../../../assets/icons/split-20.svg?raw'
import terminalIcon from '../../../assets/icons/terminal-20.svg?raw'
import globeIcon from '../../../assets/icons/globe-16.svg?raw'
import type { Localized } from '../engine/i18n.ts'

export interface Feature {
  icon: string
  // The feature chip's text (shooting script, section 5).
  name: Localized
}

export const FEATURES = {
  crew: { icon: splitIcon, name: { en: 'Crew', zh: '工作会话' } },
  edits: { icon: pencilIcon, name: { en: 'Edits', zh: '改文件' } },
  retries: { icon: retryIcon, name: { en: 'Retries', zh: '重试' } },
  questions: { icon: questionIcon, name: { en: 'Questions', zh: '提问' } },
  permissions: { icon: lockIcon, name: { en: 'Permissions', zh: '授权' } },
  delay: { icon: clockIcon, name: { en: 'Tool delays', zh: '工具延迟' } },
  windows: { icon: globeIcon, name: { en: 'Three windows', zh: '三个窗口' } },
  opencode: { icon: slidersIcon, name: { en: 'opencode.json', zh: '沿用配置' } },
  backend: { icon: terminalIcon, name: { en: 'ref-server', zh: '独立后端' } },
} as const satisfies Record<string, Feature>
