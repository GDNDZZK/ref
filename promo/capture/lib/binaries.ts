import { existsSync } from 'node:fs'
import { join, resolve } from 'node:path'

export const REPO_ROOT = resolve(import.meta.dirname, '..', '..', '..')

// REF_BIN_DIR points at a directory with ref, ref-server and ref-web; default is the
// newest cargo build of this checkout.
function binDir(): string {
  if (process.env.REF_BIN_DIR) return process.env.REF_BIN_DIR
  const candidates = ['release', 'debug'].map((profile) => join(REPO_ROOT, 'target', profile))
  const found = candidates.find((dir) => existsSync(join(dir, 'ref-server')))
  if (!found) throw new Error('no ref binaries: run `cargo build` (after `pnpm build` in web/) or set REF_BIN_DIR')
  return found
}

export function binary(name: 'ref' | 'ref-server' | 'ref-web'): string {
  return join(binDir(), name)
}
