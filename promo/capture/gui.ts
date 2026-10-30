import type { Lang } from '../src/engine/i18n.ts'
import { withGuiUi } from './lib/guiUi.ts'
import type { Scenario } from './lib/ui.ts'

export async function captureGui(scenarios: Scenario[], langs: Lang[]): Promise<void> {
  for (const scenario of scenarios) {
    for (const lang of langs) {
      console.log(`gui ${scenario.name} ${lang}`)
      await withGuiUi(lang, (ui) => scenario.run(ui))
    }
  }
}
