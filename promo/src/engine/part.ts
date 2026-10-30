// A Part is one visual element of a shot. `render(time)` puts it in the state it has at
// `time` seconds after the shot started, and does not depend on any earlier call.
export interface Part {
  readonly el: HTMLElement
  render(time: number): void | Promise<void>
}

export function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
  parent?: HTMLElement,
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag)
  el.className = className
  parent?.append(el)
  return el
}
