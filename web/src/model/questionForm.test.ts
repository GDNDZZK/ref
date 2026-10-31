import { describe, expect, it } from 'vitest'
import type { Question, QuestionRequest } from '../proto/generated'
import { QuestionForm } from './questionForm.ts'

function question(overrides: Partial<Question> = {}): Question {
  return {
    question: 'How should it be fixed?',
    header: 'Fix',
    options: [
      { label: 'Range', description: 'exclusive', default: false },
      { label: 'Iterator', description: 'iter()', default: true },
      { label: 'Guard', description: 'check', default: false },
    ],
    multiple: false,
    custom: true,
    ...overrides,
  }
}

function formOf(...questions: Question[]): QuestionForm {
  const request: QuestionRequest = { session_id: 's1', message_id: 'm1', call_id: 'c1', questions, time: 0 }
  return new QuestionForm(request)
}

describe('a single question with a single choice', () => {
  it('starts on the default option, which counts as answered', () => {
    const form = formOf(question())
    expect(form.tabbed).toBe(false)
    expect(form.choices[0]!.cursor).toBe(1)
    expect(form.answers()).toEqual([['Iterator']])
  })

  it('takes the highlighted row with Enter', () => {
    const form = formOf(question())
    expect(form.press('Enter')).toBe('submit')
    expect(form.answers()).toEqual([['Iterator']])
  })

  it('moves the highlight with the arrows, round the list including the custom row', () => {
    const form = formOf(question())
    form.press('ArrowUp')
    form.press('ArrowUp')
    expect(form.choices[0]!.cursor).toBe(3)
    form.press('ArrowDown')
    expect(form.choices[0]!.cursor).toBe(0)
    expect(form.press('Enter')).toBe('submit')
    expect(form.answers()).toEqual([['Range']])
  })

  it('answers with a number key', () => {
    const form = formOf(question())
    expect(form.press('3')).toBe('submit')
    expect(form.answers()).toEqual([['Guard']])
  })

  it('ignores a number past the last row', () => {
    const form = formOf(question({ custom: false }))
    expect(form.press('4')).toBe('ignored')
    expect(form.press('9')).toBe('ignored')
  })

  it('opens the custom field instead of submitting on the custom row, then submits its text', () => {
    const form = formOf(question())
    expect(form.press('4')).toBe('handled')
    expect(form.editing).toBe(true)
    expect(form.commitCustom()).toBe('handled') // nothing typed yet
    form.setCustomText(0, '  Use a while loop ')
    expect(form.commitCustom()).toBe('submit')
    expect(form.answers()).toEqual([['Use a while loop']])
  })

  it('goes back to the list with Esc in the field and drops an empty custom answer', () => {
    const form = formOf(question())
    form.press('4')
    form.leaveCustom()
    expect(form.editing).toBe(false)
    expect(form.answers()).toEqual([[]])
  })

  it('has no default selected when none is marked', () => {
    const form = formOf(question({ options: question().options.map((o) => ({ ...o, default: false })) }))
    expect(form.answers()).toEqual([[]])
    expect(form.choices[0]!.cursor).toBe(0)
  })

  it('dismisses with Esc', () => {
    expect(formOf(question()).press('Escape')).toBe('dismiss')
  })

  it('ignores other keys', () => {
    expect(formOf(question()).press('x')).toBe('ignored')
  })
})

describe('several questions or a multiple choice', () => {
  const checks = question({
    question: 'Which checks?',
    header: 'Checks',
    multiple: true,
    options: [
      { label: 'Tests', description: '', default: true },
      { label: 'Clippy', description: '', default: true },
      { label: 'Fmt', description: '', default: false },
    ],
  })

  it('has a tab per question and a confirm tab', () => {
    const form = formOf(question(), checks)
    expect(form.tabbed).toBe(true)
    expect(form.tabCount).toBe(3)
    expect(formOf(checks).tabCount).toBe(2)
  })

  it('starts with the defaults checked', () => {
    expect(formOf(question(), checks).answers()).toEqual([['Iterator'], ['Tests', 'Clippy']])
  })

  it('picking in a single choice moves to the next tab', () => {
    const form = formOf(question(), checks)
    expect(form.press('1')).toBe('handled')
    expect(form.answers()[0]).toEqual(['Range'])
    expect(form.tab).toBe(1)
  })

  it('flips checkboxes with number keys and Space, and Enter moves on', () => {
    const form = formOf(checks)
    form.press('1') // unchecks Tests
    form.press('3') // checks Fmt; the cursor is on row 3
    expect(form.answers()).toEqual([['Clippy', 'Fmt']])
    form.press('ArrowUp') // row 2 (Clippy)
    form.press(' ')
    expect(form.answers()).toEqual([['Fmt']])
    expect(form.tab).toBe(0)
    expect(form.press('Enter')).toBe('handled')
    expect(form.onConfirmTab).toBe(true)
  })

  it('keeps a checked custom answer after the labels', () => {
    const form = formOf(checks)
    form.press('4')
    expect(form.editing).toBe(true)
    form.setCustomText(0, 'Doc tests')
    expect(form.commitCustom()).toBe('handled')
    expect(form.answers()).toEqual([['Tests', 'Clippy', 'Doc tests']])
  })

  it('walks the tabs with the arrows and Tab', () => {
    const form = formOf(question(), checks)
    form.press('ArrowRight')
    expect(form.tab).toBe(1)
    form.press('Tab')
    expect(form.onConfirmTab).toBe(true)
    form.press('Tab')
    expect(form.tab).toBe(0)
    form.press('Tab', true)
    expect(form.onConfirmTab).toBe(true)
    form.press('ArrowLeft')
    expect(form.tab).toBe(1)
  })

  it('submits from the confirm tab and lists the questions left unanswered', () => {
    const noDefault = question({ options: question().options.map((o) => ({ ...o, default: false })) })
    const form = formOf(noDefault, checks)
    form.goToTab(2)
    expect(form.unansweredQuestions()).toEqual([0])
    expect(form.press('1')).toBe('ignored')
    expect(form.press('Enter')).toBe('submit')
  })

  it('does not go past the tabs', () => {
    const form = formOf(question(), checks)
    form.goToTab(9)
    expect(form.tab).toBe(2)
    form.goToTab(-3)
    expect(form.tab).toBe(0)
  })

  it('answers a single choice in a tabbed form with the custom text', () => {
    const form = formOf(question(), checks)
    form.press('4')
    form.setCustomText(0, 'mine')
    form.commitCustom()
    expect(form.answers()[0]).toEqual(['mine'])
    expect(form.tab).toBe(1)
  })

  it('dismisses with Esc on any tab', () => {
    const form = formOf(question(), checks)
    form.goToTab(2)
    expect(form.press('Escape')).toBe('dismiss')
  })
})
