// What the user has chosen so far for one `question` tool call, and the keys that change it.
//
// One question with a single choice is a plain list: picking a row answers it. Anything else
// (several questions, or a multiple choice) has a tab per question and a last tab that
// confirms. Pure TypeScript: the card and the key handler both work on the same form.

import type { Question, QuestionRequest } from '../proto/generated'

// What a key asks the owner of the form to do next.
export type FormOutcome = 'submit' | 'dismiss' | 'handled' | 'ignored'

interface QuestionChoice {
  cursor: number // the highlighted row
  picked: boolean[] // per option
  customPicked: boolean // the "type your own answer" row is chosen
  customText: string
}

const FIRST_NUMBER_KEY = 1
const MAX_NUMBER_KEY = 9

export class QuestionForm {
  readonly request: QuestionRequest
  tab = 0 // the question shown; `questions.length` is the confirm tab
  editing = false // the cursor is in the text of the custom answer
  readonly choices: QuestionChoice[]

  constructor(request: QuestionRequest) {
    this.request = request
    this.choices = request.questions.map(initialChoice)
  }

  get questions(): readonly Question[] {
    return this.request.questions
  }

  // Several questions or a multiple choice: tabs and a confirm step. Otherwise picking answers.
  get tabbed(): boolean {
    return this.questions.length > 1 || this.questions[0]?.multiple === true
  }

  get onConfirmTab(): boolean {
    return this.tabbed && this.tab >= this.questions.length
  }

  get tabCount(): number {
    return this.tabbed ? this.questions.length + 1 : 1
  }

  // Options, then the custom answer when allowed.
  rowCount(index: number): number {
    const question = this.questions[index]
    return question ? question.options.length + (question.custom ? 1 : 0) : 0
  }

  isCustomRow(index: number, row: number): boolean {
    return row === this.questions[index]?.options.length
  }

  isChosen(index: number, row: number): boolean {
    const choice = this.choices[index]
    if (!choice) return false
    return this.isCustomRow(index, row) ? choice.customPicked : choice.picked[row] === true
  }

  // One list of labels per question; empty for a question the user left alone.
  answers(): string[][] {
    return this.questions.map((question, index) => {
      const choice = this.choices[index]!
      const labels = question.options.filter((_option, row) => choice.picked[row]).map((o) => o.label)
      const text = choice.customText.trim()
      return choice.customPicked && text !== '' ? [...labels, text] : labels
    })
  }

  unansweredQuestions(): number[] {
    return this.answers().flatMap((answer, index) => (answer.length === 0 ? [index] : []))
  }

  goToTab(tab: number): void {
    this.editing = false
    this.tab = Math.max(0, Math.min(tab, this.tabCount - 1))
  }

  moveTab(delta: number): void {
    this.goToTab((this.tab + delta + this.tabCount) % this.tabCount)
  }

  highlight(index: number, row: number): void {
    const choice = this.choices[index]
    if (choice) choice.cursor = row
  }

  moveCursor(index: number, delta: number): void {
    const rows = this.rowCount(index)
    const choice = this.choices[index]
    if (choice && rows > 0) choice.cursor = (choice.cursor + delta + rows) % rows
  }

  // Chooses a row of a single choice, or flips it in a multiple choice. The custom row starts
  // editing when it becomes chosen.
  select(index: number, row: number): void {
    const question = this.questions[index]
    const choice = this.choices[index]
    if (!question || !choice || row < 0 || row >= this.rowCount(index)) return
    choice.cursor = row
    const custom = this.isCustomRow(index, row)

    if (question.multiple) {
      if (custom) choice.customPicked = !choice.customPicked
      else choice.picked[row] = !choice.picked[row]
    } else {
      choice.picked = choice.picked.map((_picked, option) => option === row)
      choice.customPicked = custom
    }
    this.editing = custom && choice.customPicked
  }

  // The text of the custom answer is done (Enter in the field).
  commitCustom(): FormOutcome {
    const choice = this.choices[this.tab]
    if (!choice) return 'ignored'
    if (choice.customText.trim() === '') return 'handled' // nothing typed: stay in the field
    this.editing = false
    return this.advance()
  }

  // Esc in the field: back to the list. With no text typed the custom row is not chosen.
  leaveCustom(): void {
    const choice = this.choices[this.tab]
    this.editing = false
    if (choice && choice.customText.trim() === '') choice.customPicked = false
  }

  setCustomText(index: number, text: string): void {
    const choice = this.choices[index]
    if (choice) choice.customText = text
  }

  // A key press outside the text field.
  press(key: string, shift = false): FormOutcome {
    if (key === 'Escape') return 'dismiss'
    if (this.onConfirmTab) return this.pressOnConfirm(key)
    const index = this.tab
    switch (key) {
      case 'ArrowUp':
        this.moveCursor(index, -1)
        return 'handled'
      case 'ArrowDown':
        this.moveCursor(index, 1)
        return 'handled'
      case 'Enter':
        return this.pick(index, this.choices[index]!.cursor, true)
      case ' ':
        return this.pick(index, this.choices[index]!.cursor, false)
      default:
        return this.pressOther(key, shift)
    }
  }

  private pressOther(key: string, shift: boolean): FormOutcome {
    if (this.tabbed) {
      if (key === 'ArrowRight' || (key === 'Tab' && !shift)) return this.moveTabBy(1)
      if (key === 'ArrowLeft' || (key === 'Tab' && shift)) return this.moveTabBy(-1)
    }
    const number = Number(key)
    const isNumberKey = Number.isInteger(number) && number >= FIRST_NUMBER_KEY && number <= MAX_NUMBER_KEY
    if (isNumberKey) return this.pick(this.tab, number - FIRST_NUMBER_KEY, false)
    return 'ignored'
  }

  private pressOnConfirm(key: string): FormOutcome {
    if (key === 'Enter') return 'submit'
    if (key === 'ArrowRight' || key === 'Tab') return this.moveTabBy(1)
    if (key === 'ArrowLeft') return this.moveTabBy(-1)
    return 'ignored'
  }

  private moveTabBy(delta: number): FormOutcome {
    this.moveTab(delta)
    return 'handled'
  }

  // A row picked with the keyboard or the mouse. `enter` is Enter, which moves on in a multiple
  // choice instead of flipping the row.
  pick(index: number, row: number, enter: boolean): FormOutcome {
    const question = this.questions[index]
    if (!question || row < 0 || row >= this.rowCount(index)) return 'ignored'
    if (question.multiple && enter) return this.advance()
    this.select(index, row)
    if (question.multiple || this.editing) return 'handled'
    return this.advance()
  }

  // Past a finished question: submit a lone single choice, otherwise show the next tab.
  private advance(): FormOutcome {
    if (!this.tabbed) return 'submit'
    this.goToTab(this.tab + 1)
    return 'handled'
  }
}

function initialChoice(question: Question): QuestionChoice {
  const picked = question.options.map((option) => option.default)
  const firstDefault = picked.indexOf(true)
  return {
    cursor: Math.max(0, firstDefault),
    picked,
    customPicked: false,
    customText: '',
  }
}
