import { translate } from '../i18n/index.ts'
import type { Label } from './types.ts'

// Call while rendering so the text follows the language.
export function labelText(label: Label): string {
  return 'key' in label ? translate(label.key) : label.text
}
