import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import { reactive } from 'vue'
import { createAppI18n } from '../i18n/index.ts'
import { QuestionForm } from '../model/questionForm.ts'
import { question as baseQuestion, questionRequest } from '../model/testMessages.ts'
import type { Question } from '../proto/generated'
import ToolQuestion from './ToolQuestion.vue'

function question(overrides: Partial<Question> = {}): Question {
  return baseQuestion({
    question: 'How should the loop be fixed?',
    header: 'Loop bound',
    options: [
      { label: 'Range', description: 'Use 0..len', default: true },
      { label: 'Iterator', description: 'Use iter()', default: false },
    ],
    ...overrides,
  })
}

function mountCard(...questions: Question[]) {
  const form = reactive(new QuestionForm(questionRequest(questions))) as QuestionForm
  const wrapper = mount(ToolQuestion, {
    props: { form, sending: false },
    global: { plugins: [createAppI18n(['en'])] },
    attachTo: document.body,
  })
  return { wrapper, form }
}

describe('ToolQuestion', () => {
  it('shows the title, the question, the options and the default mark', () => {
    const { wrapper } = mountCard(question())
    expect(wrapper.get('.title').text()).toBe('? Loop bound')
    expect(wrapper.get('.text').text()).toBe('How should the loop be fixed?')
    const options = wrapper.findAll('.option')
    expect(options.map((option) => option.get('.label').text())).toEqual([
      'Range',
      'Iterator',
      'Type your own answer',
    ])
    expect(options[0]!.text()).toContain('(default)')
    expect(options[0]!.classes()).toContain('cursor')
    expect(wrapper.find('.tabs').exists()).toBe(false)
  })

  it('answers when an option of a lone single choice is clicked', async () => {
    const { wrapper } = mountCard(question())
    await wrapper.findAll('.option')[1]!.trigger('click')
    expect(wrapper.emitted('answer')).toEqual([[[['Iterator']]]])
  })

  it('opens a field for the own answer and sends its text on Enter', async () => {
    const { wrapper, form } = mountCard(question())
    await wrapper.findAll('.option')[2]!.trigger('click')
    expect(wrapper.emitted('answer')).toBeUndefined()
    const field = wrapper.get('input.field')
    expect(document.activeElement).toBe(field.element)
    await field.setValue('A while loop')
    await field.trigger('keydown', { key: 'Enter' })
    expect(form.answers()).toEqual([['A while loop']])
    expect(wrapper.emitted('answer')).toEqual([[[['A while loop']]]])
  })

  it('leaves the field with Esc without answering', async () => {
    const { wrapper, form } = mountCard(question())
    await wrapper.findAll('.option')[2]!.trigger('click')
    await wrapper.get('input.field').trigger('keydown', { key: 'Escape' })
    expect(form.editing).toBe(false)
    expect(wrapper.emitted('answer')).toBeUndefined()
    expect(wrapper.find('input.field').exists()).toBe(false)
  })

  it('has tabs, checkboxes and a confirm page for several questions', async () => {
    const checks = question({
      header: 'Checks',
      multiple: true,
      options: [
        { label: 'Tests', description: '', default: true },
        { label: 'Fmt', description: '', default: false },
      ],
    })
    const { wrapper, form } = mountCard(question(), checks)
    const tabs = wrapper.findAll('.tab')
    expect(tabs.map((tab) => tab.text())).toEqual(['Loop bound', 'Checks', 'Confirm'])

    await tabs[1]!.trigger('click')
    expect(wrapper.findAll('.option')[0]!.attributes('aria-checked')).toBe('true')
    await wrapper.findAll('.option')[1]!.trigger('click')
    expect(form.answers()[1]).toEqual(['Tests', 'Fmt'])
    expect(wrapper.emitted('answer')).toBeUndefined()

    await tabs[2]!.trigger('click')
    expect(wrapper.findAll('.summary li').map((line) => line.text())).toEqual([
      'Loop boundRange',
      'ChecksTests, Fmt',
    ])
    await wrapper.get('.action.primary').trigger('click')
    expect(wrapper.emitted('answer')).toEqual([[[['Range'], ['Tests', 'Fmt']]]])
  })

  it('flags a question nobody answered on the confirm page', async () => {
    const none = question({ options: question().options.map((option) => ({ ...option, default: false })) })
    const { wrapper } = mountCard(none, question())
    await wrapper.findAll('.tab')[2]!.trigger('click')
    const lines = wrapper.findAll('.summary li')
    expect(lines[0]!.classes()).toContain('unanswered')
    expect(lines[0]!.text()).toContain('Unanswered')
  })

  it('dismisses from the button', async () => {
    const { wrapper } = mountCard(question())
    await wrapper.get('.action').trigger('click')
    expect(wrapper.emitted('answer')).toEqual([[null]])
  })

  it('renders a compact answered summary when form is gone and answered is provided', () => {
    const wrapper = mount(ToolQuestion, {
      props: {
        form: undefined,
        sending: false,
        answered: [
          { header: 'Loop bound', answer: 'Range' },
          { header: 'Checks', answer: '' },
        ],
      },
      global: { plugins: [createAppI18n(['en'])] },
    })
    expect(wrapper.find('.question.open').exists()).toBe(false)
    const lines = wrapper.findAll('.summary.answered li')
    expect(lines.map((line) => line.text())).toEqual(['Loop boundRange', 'ChecksUnanswered'])
    expect(lines[1]!.classes()).toContain('unanswered')
  })

  it('ignores stationary mousemove events so keyboard cursor is not hijacked', async () => {
    const { wrapper, form } = mountCard(question())
    const options = wrapper.findAll('.option')
    await options[1]!.trigger('mousemove', { clientX: 50, clientY: 60 })
    expect(form.choices[0]!.cursor).toBe(1)
    form.highlight(0, 2) // moves cursor to row 2 via keyboard
    expect(form.choices[0]!.cursor).toBe(2)
    // Stationary mousemove at same coordinates does not pull cursor back to row 1.
    await options[1]!.trigger('mousemove', { clientX: 50, clientY: 60 })
    expect(form.choices[0]!.cursor).toBe(2)
    // Genuine movement updates cursor.
    await options[1]!.trigger('mousemove', { clientX: 55, clientY: 62 })
    expect(form.choices[0]!.cursor).toBe(1)
    wrapper.unmount()
  })
})
