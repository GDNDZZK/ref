import assert from 'node:assert/strict'
import { test } from 'node:test'
import { typedPrefix } from './typedText.ts'

test('typedPrefix reveals characters at the given rate', () => {
  assert.equal(typedPrefix('hello', -1, 10), '')
  assert.equal(typedPrefix('hello', 0.25, 10), 'he')
  assert.equal(typedPrefix('hello', 9, 10), 'hello')
})

test('typedPrefix counts whole characters, not UTF-16 units', () => {
  assert.equal(typedPrefix('修复🙂测试', 0.3, 10), '修复🙂')
})
