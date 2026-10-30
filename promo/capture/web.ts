import { launchChromium } from '../scripts/lib/browser.ts'
import type { Lang } from '../src/engine/i18n.ts'
import type { Scenario } from './lib/ui.ts'
import { withWebUi } from './lib/webUi.ts'

export async function captureWeb(scenarios: Scenario[], langs: Lang[]): Promise<void> {
  const browser = await launchChromium()
  try {
    for (const scenario of scenarios) {
      for (const lang of langs) {
        console.log(`web ${scenario.name} ${lang}`)
        await withWebUi(browser, lang, (ui) => scenario.run(ui))
      }
    }
  } finally {
    await browser.close()
  }
}
