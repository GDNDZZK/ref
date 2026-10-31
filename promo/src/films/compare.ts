// A8: the comparison card, cell by cell from the shooting script section 3.2.
import type { CompareCardOptions } from '../components/compareCard.ts'

export const COMPARE: Omit<CompareCardOptions, 'from' | 'to'> = {
  title: {
    en: 'Several sessions at once: what the docs say (Oct 2026)',
    zh: '多个会话同时运行：各家文档怎么写（2026 年 10 月）',
  },
  columns: ['ref', 'opencode', 'pi', 'Claude Code', 'Codex'],
  rows: [
    {
      label: { en: 'Sessions the main agent starts: see them live', zh: '主智能体开的会话：实时查看' },
      cells: [
        { en: 'One tile per session, all streaming', zh: '每个会话一个分块，同时输出' },
        { en: 'Switch between child sessions', zh: '在子会话之间切换' },
        { en: 'No built-in subagents', zh: '没有内置子智能体' },
        { en: 'Open from /tasks; split panes: experimental', zh: '从 /tasks 打开；分屏为实验功能' },
        { en: 'Open a subagent thread', zh: '打开子智能体线程' },
      ],
    },
    {
      label: { en: 'Where a worker asks for permission', zh: '工作会话要授权时在哪里点' },
      cells: [
        { en: 'Click Allow in its own tile', zh: '在它自己的分块里点允许' },
        '—',
        { en: 'No permission system', zh: '没有授权系统' },
        { en: 'Main session; agent view: attach first', zh: '在主会话；agent view 里要先进入会话' },
        '—',
      ],
    },
  ],
  footer: {
    en: "Source: each project's docs, read 2026-10-30/31. — = not described in the project's docs as of Oct 2026",
    zh: '来源：各项目文档，2026 年 10 月 30-31 日查阅。— = 截至 2026 年 10 月官方文档未说明',
  },
  headerSeconds: 1.5,
  rowSeconds: 3.7,
}
