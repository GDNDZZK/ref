import type { ScenePlayer } from './player.ts'

// Preview aid for `pnpm dev` (?debug=1): a time slider and a play button. Rendering never uses it.
export function addDebugControls(player: ScenePlayer): void {
  const bar = document.createElement('div')
  bar.className = 'debug-bar'
  const play = document.createElement('button')
  play.textContent = 'play'
  const slider = document.createElement('input')
  slider.type = 'range'
  slider.min = '0'
  slider.max = String(player.duration)
  slider.step = '0.01'
  slider.value = '0'
  const label = document.createElement('span')
  bar.append(play, slider, label)
  document.body.append(bar)

  const show = (time: number) => {
    slider.value = String(time)
    label.textContent = `${time.toFixed(2)} / ${player.duration.toFixed(2)} s`
    void player.seek(time)
  }
  slider.addEventListener('input', () => show(Number(slider.value)))

  let playing = false
  let playStartedAt = 0
  const tick = (now: number) => {
    if (!playing) return
    const elapsed = (now - playStartedAt) / 1000
    if (elapsed >= player.duration) playing = false
    show(Math.min(elapsed, player.duration))
    requestAnimationFrame(tick)
  }
  play.addEventListener('click', () => {
    playing = !playing
    play.textContent = playing ? 'pause' : 'play'
    if (!playing) return
    playStartedAt = performance.now() - Number(slider.value) * 1000
    requestAnimationFrame(tick)
  })
  show(0)
}

// Scales the 1920x1080 stage to the browser window while previewing.
export function fitStageToWindow(): void {
  const fit = () => {
    const scale = Math.min(window.innerWidth / 1920, (window.innerHeight - 40) / 1080)
    document.documentElement.style.setProperty('--preview-scale', String(scale))
  }
  window.addEventListener('resize', fit)
  fit()
  document.documentElement.dataset.preview = ''
}
