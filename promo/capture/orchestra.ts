// The `orchestrate` mock script (docs/development.md#the-mock-backend): a main session starts
// four helpers, then a reviewer, and ends with a summary about 25 s later. Two runs, because a
// clip of the crowd and the pictures of the main conversation need different views:
//   orchestra-main   the task, the plan, the reports, the inbox and the summary of the main session
//   orchestra-crowd  the split view as a clip, with the helper that waits for approval
// The mock's timeline is fixed (seconds after the task is sent): helpers at 1.5/3/4.5/6, the
// fixer's edit waits for approval from 6.4, reports at 10.5/12/12.5/14, the reviewer at 16 and 21.
import type { Scenario, Ui } from './lib/ui.ts'

const TASK = 'orchestrate fix the parser bug, test it, update the docs'
const PLAN_DONE = 'Four helpers are working'
const SUMMARY_DONE = 'The parser bug is fixed.'
const WAIT_MS = 40_000
const MAIN_SESSION_ROW = 'workspace-' // the main session keeps its generated name; helpers are named
const FIXER = 'fixer'
const TESTER = 'tester'
const FOLLOW_UP = 'also cover empty input'
const TYPING_PAUSE_MS = 70
const TESTER_REACTION = 'Also covered: empty input.'
const ALLOW_KEY = 'tool.allow'
const APPROVAL_ASKED_FOR = 'Fix the off-by-one' // the fixer's edit

// Seconds since `start`, as a pause that returns at that moment.
function clock(ui: Ui) {
  const start = Date.now()
  return {
    until: (seconds: number) => ui.pause(Math.max(0, start + seconds * 1000 - Date.now())),
    now: () => (Date.now() - start) / 1000,
  }
}

export const ORCHESTRA_SCENARIOS: Scenario[] = [
  {
    name: 'orchestra-main',
    uis: ['gui'],
    async run(ui) {
      await ui.type(TASK)
      await ui.pause(600)
      await ui.shot('orchestra-task')
      await ui.key('Enter')
      const time = clock(ui)

      await ui.waitText(PLAN_DONE, WAIT_MS)
      await ui.pause(700)
      await ui.shot('orchestra-plan')

      // The fixer's edit waits for approval in its own session.
      await time.until(8.2)
      await ui.clickSidebarRow(FIXER)
      await ui.pause(900)
      await ui.key('a')
      await ui.pause(300)
      await ui.clickText(MAIN_SESSION_ROW)

      await ui.parkPointer()
      await time.until(13.8)
      await ui.shot('orchestra-reports')
      await ui.send('/peers-inbox')
      await ui.pause(1200)
      await ui.shot('orchestra-inbox')
      await ui.key('Escape')
      await ui.parkPointer()

      await ui.waitText(SUMMARY_DONE, WAIT_MS)
      await ui.pause(900)
      await ui.shot('orchestra-done')
    },
  },
  {
    name: 'orchestra-crowd',
    uis: ['gui'],
    async run(ui) {
      await ui.resize(1280, 1120)
      await ui.type(TASK)
      await ui.key('Enter')
      const time = clock(ui)
      await time.until(0.4)
      await ui.command('watch')
      await ui.startClip('orchestra-crowd')

      await time.until(7.5)
      await ui.key('Alt+Down') // the fixer's tile is under the main one
      await time.until(8.5)
      await time.until(9)
      await ui.clickTextBelow(ui.text(ALLOW_KEY), APPROVAL_ASKED_FOR)
      await time.until(13)
      await ui.endClip()
    },
  },
  {
    // The clip takes long to encode, so the picture of the full grid comes from a second run.
    name: 'orchestra-grid',
    uis: ['gui'],
    async run(ui) {
      await ui.resize(1280, 1120)
      await ui.type(TASK)
      await ui.key('Enter')
      const time = clock(ui)
      await time.until(0.4)
      await ui.command('watch')
      await time.until(7.5)
      await ui.key('Alt+Down')
      await time.until(9)
      await ui.clickTextBelow(ui.text(ALLOW_KEY), APPROVAL_ASKED_FOR)
      await time.until(17.5)
      await ui.shot('orchestra-six')
    },
  },
  {
    // Test of the hero shot: a message queued for the tester while the fixer waits for approval.
    name: 'orchestra-probe',
    uis: ['gui'],
    async run(ui) {
      await ui.resize(1280, 1120)
      await ui.type(TASK)
      await ui.key('Enter')
      const time = clock(ui)
      await time.until(0.4)
      await ui.command('watch')
      await ui.parkPointer()
      await ui.startClip('orchestra-probe')
      await time.until(7)
      await ui.clickTile(TESTER)
      await ui.parkPointer() // the input box opens under the pointer and would show a tooltip
      const typingStart = time.now()
      for (const character of FOLLOW_UP) {
        await ui.type(character)
        await ui.pause(TYPING_PAUSE_MS)
      }
      console.log('typing took', (time.now() - typingStart).toFixed(1))
      await ui.key('Enter')
      await ui.parkPointer()
      console.log('t', time.now().toFixed(1), 'fixer still waiting:', await ui.hasText(ui.text('status.waitingPermission')))
      await time.until(13)
      await ui.clickTextBelow(ui.text(ALLOW_KEY), APPROVAL_ASKED_FOR)
      await ui.parkPointer()
      await time.until(14)
      console.log('t', time.now().toFixed(1), 'fixer still waiting after one click:', await ui.hasText(ui.text('status.waitingPermission')))
      await ui.waitText(TESTER_REACTION, WAIT_MS)
      console.log('t', time.now().toFixed(1), 'tester reaction seen')
      await ui.waitText(SUMMARY_DONE, WAIT_MS)
      console.log('t', time.now().toFixed(1), 'summary done')
      await ui.pause(1000)
      await ui.endClip()
    },
  },
]
