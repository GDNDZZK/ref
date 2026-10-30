import type { Part } from './part.ts'
import { element } from './part.ts'

export interface ShotBuilder {
  readonly root: HTMLElement
  add<P extends Part>(part: P): P
}

export interface ShotInstance {
  readonly root: HTMLElement
  readonly duration: number
  render(time: number): Promise<void>
}

// A shot is a self-contained stretch of a scene: an opaque stage layer with parts on it.
export interface Shot {
  readonly duration: number
  mount(): ShotInstance
}

export function defineShot(duration: number, build: (shot: ShotBuilder) => void): Shot {
  return {
    duration,
    mount() {
      const root = element('div', 'shot')
      const parts: Part[] = []
      build({
        root,
        add(part) {
          root.append(part.el)
          parts.push(part)
          return part
        },
      })
      return {
        root,
        duration,
        async render(time) {
          await Promise.all(parts.map((part) => part.render(time)))
        },
      }
    },
  }
}
