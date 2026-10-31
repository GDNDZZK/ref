import bellIcon from '../../../assets/icons/bell-20.svg?raw'
import globeIcon from '../../../assets/icons/globe-16.svg?raw'
import lockIcon from '../../../assets/icons/lock-16.svg?raw'
import pencilIcon from '../../../assets/icons/pencil-16.svg?raw'
import slashIcon from '../../../assets/icons/slash-16.svg?raw'
import slidersIcon from '../../../assets/icons/sliders-20.svg?raw'
import splitIcon from '../../../assets/icons/split-20.svg?raw'
import historyIcon from '../../../assets/icons/history-20.svg?raw'
import chatSendIcon from '../../../assets/icons/chat-send-16.svg?raw'
import type { Localized } from '../engine/i18n.ts'

export interface Feature {
  id: string
  icon: string
  name: Localized
  tagline: Localized
}

// The headline features, in trailer order (`orchestra` is the hero film). Names are the tag on screen; taglines
// are the title card of the feature's own film.
export const FEATURES = {
  orchestra: {
    id: 'orchestra',
    icon: splitIcon,
    name: { en: 'One session, a whole crew', zh: '一个会话，一整队帮手' },
    tagline: { en: 'Hand over one task. Helper sessions do the work and report back.', zh: '交出一个任务，帮手会话分头干活，再回来汇报。' },
  },
  backend: {
    id: 'backend',
    icon: globeIcon,
    name: { en: 'One backend', zh: '一个后端' },
    tagline: { en: 'Terminal, desktop and browser show the same sessions.', zh: '终端、桌面、浏览器，看到的是同一批会话。' },
  },
  sessions: {
    id: 'sessions',
    icon: splitIcon,
    name: { en: 'Many sessions', zh: '多会话' },
    tagline: { en: 'Run several at once. Watch them all.', zh: '同时运行多个，一眼看全。' },
  },
  peers: {
    id: 'peers',
    icon: chatSendIcon,
    name: { en: 'Sessions that talk', zh: '会话互通' },
    tagline: { en: 'Sessions message each other and start helpers.', zh: '会话互发消息，还能启动帮手。' },
  },
  permissions: {
    id: 'permissions',
    icon: lockIcon,
    name: { en: 'You decide', zh: '你说了算' },
    tagline: { en: 'Allow, ask or deny every tool call.', zh: '每次工具调用，允许、询问或拒绝。' },
  },
  edits: {
    id: 'edits',
    icon: pencilIcon,
    name: { en: 'Safe edits', zh: '安全修改' },
    tagline: { en: 'Edits address tagged lines, so stale views are caught.', zh: '按带标签的行号修改，过期视图会被发现。' },
  },
  composer: {
    id: 'composer',
    icon: slashIcon,
    name: { en: 'The prompt box', zh: '输入框' },
    tagline: { en: 'Commands, files, shell, images, queue and undo.', zh: '命令、文件、命令行、图片、排队与撤销。' },
  },
  longSessions: {
    id: 'long-sessions',
    icon: historyIcon,
    name: { en: 'Long sessions', zh: '长会话' },
    tagline: { en: 'Compaction and retries keep a session going.', zh: '自动压缩与重试，让会话一直跑下去。' },
  },
  setup: {
    id: 'setup',
    icon: slidersIcon,
    name: { en: 'Your setup', zh: '你的配置' },
    tagline: { en: 'Bring your opencode config, any model, MCP.', zh: '沿用 opencode 配置，任意模型，MCP。' },
  },
  notify: {
    id: 'notify',
    icon: bellIcon,
    name: { en: 'Notifications', zh: '通知' },
    tagline: { en: 'Called back when a session needs you.', zh: '会话需要你时，会通知你。' },
  },
} as const satisfies Record<string, Feature>
