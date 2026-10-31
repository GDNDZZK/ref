// pnpm capture [web|gui|tui|all] [--only name,name] [--lang en,zh]
// Records pictures and casts from the mock backend into assets/ (and docs/images/).
import { spawnSync } from 'node:child_process'
import { parseArgs } from 'node:util'
import { LANGS, isLang, type Lang } from '../src/engine/i18n.ts'
import { captureGui } from './gui.ts'
import { captureCrew } from './crew.ts'
import { captureShared } from './shared.ts'
import { SCENARIOS } from './scenarios.ts'
import { PRIVATE_BUS_FLAG } from './lib/privateBus.ts'
import { captureTui } from './tui.ts'
import { captureWeb } from './web.ts'

// ref-gui, ref and Chromium send desktop notifications over the session bus when a turn ends.
// Everything the capture starts runs on a private bus, never on the user's.
if (process.env[PRIVATE_BUS_FLAG] !== '1') {
  const rerun = spawnSync('dbus-run-session', ['--', process.execPath, ...process.argv.slice(1)], {
    stdio: 'inherit',
    env: { ...process.env, [PRIVATE_BUS_FLAG]: '1' },
  })
  if (rerun.error) throw new Error(`cannot start a private D-Bus session (dbus-run-session): ${rerun.error.message}`)
  process.exit(rerun.status ?? 1)
}

const TARGETS = ['web', 'gui', 'tui', 'shared', 'crew'] as const
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
  if (target === 'crew') {
    await captureCrew()
    continue
  }
  const forTarget = selected.filter((scenario) => (scenario.uis ?? ['web', 'gui']).includes(target))
  const run = target === 'web' ? captureWeb : captureGui
  await run(forTarget, langs as Lang[])
}
