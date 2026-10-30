// Long-conversation check in headless Chromium: puts synthetic blocks into the conversation store
// of a page connected to `ref-web --mock`, then measures first render, scrolling and streaming at
// the end. Software rendering, so read the numbers as relative.
//   node scripts/perf.mjs [blockCount]
import { withMockPage } from './smokeLib.mjs'

const RENDER_LIMIT_MS = 40000
const COUNT = Number(process.argv[2] ?? 5000)

function measure({ count, RENDER_LIMIT_MS }) {
  const app = document.querySelector('#app').__vue_app__
  const store = app.config.globalProperties.$pinia._s.get('conversation')
  const frames = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)))
  const sentence = 'The quick brown fox jumps over the lazy dog while the parser reads another line. '
  const base = { messageId: 'm', streaming: false }

  function makeBlocks() {
    const blocks = []
    for (let i = 0; blocks.length < count; i++) {
      const key = (kind) => `${kind}-${i}`
      blocks.push({
        ...base,
        key: key('user'),
        kind: 'user',
        text: `Question ${i}: ${sentence}`,
        source: 'human',
        senderName: undefined,
        opensTurn: i > 0,
      })
      blocks.push({ ...base, key: key('reasoning'), kind: 'reasoning', text: sentence.repeat(3) })
      blocks.push({
        ...base,
        key: key('markdown'),
        kind: 'markdown',
        text: `**Answer ${i}.** ${sentence.repeat(4)}\n\n- one\n- two\n- \`three\``,
      })
      blocks.push({
        ...base,
        key: key('code'),
        kind: 'code',
        language: 'rust',
        text: Array.from({ length: 8 }, (_, n) => `let value_${n} = compute(${n});`).join('\n'),
      })
      blocks.push({
        ...base,
        key: key('tool'),
        kind: 'tool',
        callId: `c${i}`,
        name: 'read',
        purpose: `Read file ${i}`,
        state: 'done',
        input: { path: `src/file${i}.rs` },
        rawInput: '',
        output: sentence.repeat(2),
        meta: null,
        time: 0,
        timeEnd: 1,
        runAt: null,
        permission: undefined,
      })
    }
    return blocks.slice(0, count)
  }

  function percentile(sorted, p) {
    return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * p))]
  }

  async function frameTimes(steps, onFrame) {
    const times = []
    const stepped = []
    let last = performance.now()
    for (let step = 0; step < steps; step++) {
      await new Promise((resolve) =>
        requestAnimationFrame((now) => {
          onFrame(step)
          times.push(now - last)
          stepped.push([step, now - last])
          last = now
          resolve()
        }),
      )
    }
    times.sort((a, b) => a - b)
    const slowest = stepped.sort((a, b) => b[1] - a[1]).slice(0, 3)
    return { slowest, p50: percentile(times, 0.5), p95: percentile(times, 0.95), max: times.at(-1) }
  }

  return (async () => {
    const result = {}
    while (!store.loaded) await frames() // the session opening would replace the blocks
    const blocks = makeBlocks()
    const started = performance.now()
    store.generation++
    store.blocks = blocks
    while (
      document.querySelectorAll('.frame').length < count &&
      performance.now() - started < RENDER_LIMIT_MS
    )
      await frames()
    result.rendered = document.querySelectorAll('.frame').length
    result.renderMs = performance.now() - started
    const layoutStarted = performance.now()
    result.scrollHeight = document.querySelector('.scroller').scrollHeight // forces style and layout
    result.layoutMs = performance.now() - layoutStarted
    await frames()
    result.settledMs = performance.now() - started
    result.domNodes = document.getElementsByTagName('*').length
    result.heapMB = performance.memory ? performance.memory.usedJSHeapSize / 1048576 : null

    const scroller = document.querySelector('.scroller')
    const stride = scroller.scrollHeight / 200
    result.scrollFrames = await frameTimes(200, (step) => (scroller.scrollTop = step * stride))

    scroller.scrollTop = scroller.scrollHeight
    await frames()
    let current = blocks
    result.streamFrames = await frameTimes(120, (step) => {
      current = current.slice()
      const last = current.length - 1
      current[last] = { ...current[last], streaming: true, text: current[last].text + `word${step} ` }
      store.blocks = current
    })
    return result
  })()
}

const passed = await withMockPage(async (page) => {
  const result = await page.evaluate(measure, { count: COUNT, RENDER_LIMIT_MS })
  console.log(JSON.stringify({ blocks: COUNT, ...result }, null, 2))
})
process.exit(passed ? 0 : 1)
