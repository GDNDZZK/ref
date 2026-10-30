// pnpm capture [tui|web|all]: records screenshots and casts from the mock backend.
import { captureTui } from './tui.ts'
import { captureWeb } from './web.ts'

const TARGETS = { tui: captureTui, web: captureWeb } satisfies Record<string, () => Promise<void>>
type Target = keyof typeof TARGETS

const requested = process.argv[2] ?? 'all'
const targets = requested === 'all' ? (Object.keys(TARGETS) as Target[]) : [requested as Target]
const unknown = targets.find((target) => !(target in TARGETS))
if (unknown) throw new Error(`unknown capture target "${unknown}"; known: all, ${Object.keys(TARGETS).join(', ')}`)

for (const target of targets) {
  console.log(`capturing ${target}`)
  await TARGETS[target]()
}
