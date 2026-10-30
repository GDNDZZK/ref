// pnpm capture [web|gui|tui|all] [--only name,name] [--lang en,zh]
// Records pictures and casts from the mock backend into assets/ (and docs/images/).
import { parseArgs } from 'node:util'
import { LANGS, isLang, type Lang } from '../src/engine/i18n.ts'
import { captureGui } from './gui.ts'
import { captureShared } from './shared.ts'
import { SCENARIOS } from './scenarios.ts'
import { captureTui } from './tui.ts'
import { captureWeb } from './web.ts'

const TARGETS = ['web', 'gui', 'tui', 'shared'] as const
type Target = (typeof TARGETS)[number]

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: { only: { type: 'string' }, lang: { type: 'string', default: LANGS.join(',') } },
})

const requested = positionals[0] ?? 'all'
const targets: readonly Target[] = requested === 'all' ? TARGETS : TARGETS.filter((target) => target === requested)
if (targets.length === 0) throw new Error(`unknown target "${requested}"; known: all, ${TARGETS.join(', ')}`)

const langs = values.lang.split(',')
const badLang = langs.find((lang) => !isLang(lang))
if (badLang) throw new Error(`unknown language "${badLang}"`)

const only = values.only?.split(',')
const unknownScenario = only?.find((name) => !SCENARIOS.some((scenario) => scenario.name === name))
if (unknownScenario) throw new Error(`unknown scenario "${unknownScenario}"; known: ${SCENARIOS.map((s) => s.name).join(', ')}`)
const selected = SCENARIOS.filter((scenario) => !only || only.includes(scenario.name))

for (const target of targets) {
  if (target === 'tui') {
    await captureTui()
    continue
  }
  if (target === 'shared') {
    await captureShared()
    continue
  }
  const forTarget = selected.filter((scenario) => (scenario.uis ?? ['web', 'gui']).includes(target))
  const run = target === 'web' ? captureWeb : captureGui
  await run(forTarget, langs as Lang[])
}
