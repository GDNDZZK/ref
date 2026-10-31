// A12: the release archives, one per system from the README's download table, without a version.
import type { ReleaseCardOptions } from '../components/releaseCard.ts'

const PLATFORMS = [
  [{ en: 'Linux x86_64', zh: 'Linux x86_64' }, 'x86_64-unknown-linux-gnu.tar.gz'],
  [{ en: 'Linux arm64', zh: 'Linux arm64' }, 'aarch64-unknown-linux-gnu.tar.gz'],
  [{ en: 'macOS Apple silicon', zh: 'macOS Apple 芯片' }, 'aarch64-apple-darwin.tar.gz'],
  [{ en: 'macOS Intel', zh: 'macOS Intel' }, 'x86_64-apple-darwin.tar.gz'],
  [{ en: 'Windows x86_64', zh: 'Windows x86_64' }, 'x86_64-pc-windows-msvc.zip'],
  [{ en: 'Windows arm64', zh: 'Windows arm64' }, 'aarch64-pc-windows-msvc.zip'],
] as const

export const RELEASE: Omit<ReleaseCardOptions, 'from' | 'to'> = {
  title: 'GitHub Releases',
  rows: PLATFORMS.map(([system, platform]) => ({ system, archive: `ref-all-…-${platform}` })),
  highlight: 0,
  highlightAt: 1.2,
}
