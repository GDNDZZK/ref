# promo

Source of the promo videos and the screenshots in `docs/images/`. The films are planned in [STORYBOARD.md](STORYBOARD.md). Finished videos go to `out/` and are not tracked; everything needed to render them is.

Needs Node 24, pnpm, ffmpeg and ffprobe on `PATH`, and a Chromium. Fonts come from the system (the stack of the web page: `system-ui`, Noto Sans, Noto Sans CJK for Chinese, DejaVu Sans Mono), so install them for identical output between machines.

```bash
cd promo
pnpm install
export CHROMIUM=/usr/bin/chromium      # path of any Chromium; without it Playwright's own download is used

pnpm render demo                       # out/demo.en.mp4 and out/demo.zh.mp4
pnpm render demo --lang en --fps 60    # one language, 60 fps
pnpm render demo --still 6.5           # one PNG of second 6.5, to check a frame
pnpm render:all                        # every scene in src/scenes/registry.ts
pnpm dev                               # preview: http://127.0.0.1:5173/?scene=demo&lang=en&debug=1
pnpm check                             # type check and unit tests
```

Options of `render`: `--lang en,zh`, `--fps N` (default 30), `--workers N` (browser processes, default 4), `--out DIR`, `--still SEC`.

## How rendering works

Every scene page is a pure function of time. `window.promo.seek(t)` puts the page in the state it has at `t` seconds, with no clock, no CSS animation and no transition (a global rule switches them off). `scripts/render.ts` starts the Vite dev server, opens one page per worker in its own Chromium process, and for each frame seeks, takes a screenshot over the DevTools protocol and pipes the PNG to ffmpeg. ffmpeg writes H.264 (`libx264`, CRF 16), `yuv420p`, BT.709, 1920×1080, `+faststart`. A console error or exception on the page stops the render.

Rendering runs at about 15 frames per second on a 12-core machine, so a one-minute film at 30 fps takes about two minutes.

## Layout

| Path | Contents |
|---|---|
| `src/engine/` | `anim.ts` (progress, key tracks), `shot.ts`, `scene.ts` (timeline and transitions), `player.ts` (`seek`), `i18n.ts` |
| `src/components/` | Parts shared by scenes: `backdrop`, `titleCard`, `caption`, `appFrame` (window with a camera over a screenshot), `highlight`, `cursor`, `typedText`, `terminalView` (replays a cast) |
| `src/shots/` | Shots reused by several scenes (`intro`, `outro`) |
| `src/scenes/` | One file per film, listed in `registry.ts`. `demo.ts` is the pipeline check and uses placeholder material |
| `scripts/` | `render.ts` and its helpers |
| `capture/` | `pnpm capture [tui\|web]`: screenshots and casts from the mock backend |
| `assets/` | `screens/` (PNG), `casts/` (asciicast v2), `placeholder/` |

The look comes from the product: `web/src/theme/generated/theme.css` (colours generated from `Theme.qml`), the easing curves in `web/src/motion/easing.ts`, `assets/icons` and `assets/app`.

### Writing a scene

```ts
const shot = defineShot(6, (shot) => {            // 6 seconds
  shot.add(backdrop())
  const frame = shot.add(appFrame({ image, imageSize, title: 'ref', rect, camera, from: 0, to: 6 }))
  frame.attach(highlight({ x, y, width, height, label: { en: 'Prompt', zh: '输入框' }, from: 2, to: 5 }))
  shot.add(caption([{ from: 1, to: 5, text: { en: 'One backend', zh: '一个后端' } }]))
})

export const scene: SceneDefinition = {
  id: 'name',
  steps: [{ shot: introShot('…') }, { shot, enter: { kind: 'fade', duration: 0.5 } }, { shot: outroShot() }],
}
```

Rules: times are seconds; a part's `render(t)` must not depend on earlier calls; the shot before a transition has to include the overlap in its duration; text is a string or `{ en, zh }`.

## Capturing screenshots

`pnpm capture` starts everything it needs with temporary `REF_STATE_DIR` and `REF_DATA_DIR`, on free ports, and stops only the processes it started. It uses the binaries in `target/release` or `target/debug` of this checkout, or `REF_BIN_DIR`. Build them first (`cd web && pnpm install && pnpm build`, then `cargo build`).

- `pnpm capture:tui`: runs `ref --server URL --token T` in a pseudo terminal made by `script(1)`, types and presses keys, follows the screen with a headless xterm.js (`waitForText`), records an asciicast to `assets/casts/` and renders stills with the same xterm.js and theme as the videos.
- `pnpm capture:web`: drives the WebUI with Playwright (dark and light, 1600×1000 at 2×).
- GUI (`ref-gui --mock` with `QT_QPA_PLATFORM=offscreen`, window grab): added once the GUI is finished.

Each still is written to `assets/screens/<name>.png` at full size and, for README images, to `docs/images/<name>.png` at 1440 px wide. Both are palette PNGs (`sharp`).
