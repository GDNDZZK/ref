# promo

Source of the promo videos and the screenshots in `docs/images/`. The films are planned in [STORYBOARD.md](STORYBOARD.md). Finished videos go to `out/` and are not tracked; everything needed to render them is.

Needs Node 24, pnpm, ffmpeg and ffprobe on `PATH`, and a Chromium. Fonts come from the system; none are bundled. The stack is the web page's (`system-ui`, `Noto Sans`, `Noto Sans CJK SC`) for text and `ui-monospace`, `DejaVu Sans Mono` for code and the terminal. The reference renders were made on Debian 13 where `system-ui` resolves to Noto Sans CJK SC (`/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc`) and the monospace font is DejaVu Sans Mono (`/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf`). Install the same two families (Debian: `fonts-noto-cjk`, `fonts-dejavu-core`) to get the same glyph shapes and line breaks; with other fonts the output is still correct but text widths differ. Check with `fc-match system-ui`.

```bash
cd promo
pnpm install
export CHROMIUM=/usr/bin/chromium      # path of any Chromium; without it Playwright's own download is used

pnpm render demo                       # out/demo.en.mp4 and out/demo.zh.mp4
pnpm render demo --lang en --fps 60    # one language, 60 fps
pnpm render demo --still 6.5           # one PNG of second 6.5, to check a frame
pnpm render trailer                    # one film; out/trailer.en.mp4 and out/trailer.zh.mp4
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
| `src/components/` | Parts shared by scenes: `backdrop`, `titleCard`, `caption`, `appFrame` (window with a camera over a screenshot), `highlight`, `cursor`, `typedText`, `terminalView` (replays a cast), `featureTag`, `textCard`, `notificationCard`, `browserTab`, `serverPill` |
| `src/shots/` | Shots reused by several scenes (`intro`, `outro`, `beat`) |
| `src/films/` | The nine feature films and the trailer (see below) |
| `src/scenes/` | One file per film, listed in `registry.ts`. `demo.ts` is the pipeline check and uses placeholder material |
| `scripts/` | `render.ts` and its helpers |
| `capture/` | `pnpm capture`: screenshots and casts from the mock backend (see below) |
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

`pnpm capture [web|gui|tui|shared|all] [--only name,name] [--lang en,zh]` starts everything it needs with a temporary `REF_STATE_DIR` and `REF_DATA_DIR` under `.cache/run/`, on free ports, and stops only the processes it started. The three interfaces run against `ref-server --mock`; the scripts that make it produce a scene are in `docs/development.md#the-mock-backend`.

| Part | What it does | Output |
|---|---|---|
| `web` | `ref-web` in headless Chromium (Playwright), 1600×1000 at 2×, dark theme; English and Chinese | `assets/screens/web-<shot>[-zh].png` |
| `gui` | the unchanged `ref-gui --mock` with `QT_QPA_PLATFORM=offscreen` and `QT_SCALE_FACTOR=2` (2560×1600 grab); a small Qt plugin (`capture/gui-driver/`) reads commands (click, type, key, grab) from stdin | `assets/screens/gui-<shot>[-zh].png` |
| `tui` | `ref` in a pseudo terminal; keys are typed, the screen is followed with a headless xterm.js; the output is recorded as an asciicast and rendered as stills | `assets/casts/*.cast`, `assets/screens/tui-*.png`, and `tui-*-clean.png` without the last row (it shows the random mock workspace path); the videos and `docs/images/` use the clean copies |
| `shared` | one mock backend, with `ref`, `ref-gui` and `ref-web` showing the same session | `tui-shared`, `gui-shared`, `web-shared` |

The split-view picture in `docs/images/` is the `readme-split` scenario, not `sessions`: only finished sessions and one running tool call (so no streaming caret is caught mid-chase), and a 1280×1120 GUI window so that no card is cut by the top of a tile. The films keep using `sessions`.

`web` and `gui` run the same scripts (`capture/scenarios.ts`) through one `Ui` interface, so a shot exists under both prefixes. `docs/images/` gets the shots that are marked `readme` in the scripts, 1440 px wide, as palette PNGs (`sharp`).

What the capture needs:

- `dbus-run-session` (package `dbus-daemon` or `dbus-bin`): `pnpm capture` re-runs itself on a private D-Bus session, because ref-gui, ref and Chromium send real desktop notifications to the session bus when a turn ends. Starting any captured process outside it is refused (`capture/lib/privateBus.ts`).

- Binaries in `target/release` or `target/debug` of this checkout (`REF_BIN_DIR` overrides): `cargo build -p ref-server -p ref-web -p ref-tui`, and `cd web && pnpm install && pnpm build` first for the web page.
- Chromium: `export CHROMIUM=/usr/bin/chromium`.
- For `gui`: a `ref-gui` build and the capture plugin. Both are built into `.cache/` (not tracked). Qt and the dependency prefix are the ones the GUI build uses (here `Qt=/home/zzk/app/ref-wt/deps/Qt/6.12.0/gcc_64`, `prefix=/home/zzk/app/ref-wt/deps/prefix`):

```bash
cmake -S ../gui -B .cache/gui-build -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_PREFIX_PATH="$Qt;$prefix"
cmake --build .cache/gui-build
cmake -S capture/gui-driver -B .cache/gui-driver-build -G Ninja -DCMAKE_PREFIX_PATH="$Qt"
cmake --build .cache/gui-driver-build
export REF_GUI_QML_PATH=$prefix/lib/qml   # QML modules of the dependency prefix
```

  `REF_GUI_BIN` overrides the `ref-gui` binary. No display is needed.
- The GUI draws with the system fonts, so its text depends on the machine like the rest of the render (see above).

Unverified: the capture was run on one machine (Linux, Debian 13). The drag-and-drop and paste of images into the GUI are not captured; the image shots are from the web page. The desktop notification is drawn by the film (`notificationCard`), because a headless run has none.

### Clips (moving GUI)

A scenario can record the window instead of taking one picture: `await ui.startClip('crowd')` … `await ui.endClip()` (GUI only). The capture plugin photographs the window 15 times a second into JPEG files; `capture/lib/clips.ts` resamples them to an even 15 fps and stores one WebM per clip (`assets/clips/gui-crowd.webm`, VP9 4:4:4, a few MB, plus `manifest.json`). `pnpm render` (and `pnpm clips`) unpacks the WebM files into `.cache/clips/<name>/f0001.jpg ...`, and a beat plays one with `screens: [{ clip: 'gui-crowd' }]`; the page shows the frame for each seek time. Chinese renders take `gui-crowd-zh` when it exists.

## The films

`src/films/` has one file per film (`backend`, `sessions`, `peers`, `permissions`, `edits`, `composer`, `long-sessions`, `setup`, `notify`) and `trailer`. A film is `filmScene(feature, beats)`: a title card, the beats, the closing card. A beat is either `windowBeat` (a captured window, a camera path of `looks`, `marks`, a pointer, captions) or `cardsBeat` (text cards typed out, for config files and the illustration of a rejected edit). Picture coordinates in a beat are fractions (0..1) of the screenshot; a camera look is a point and a zoom (1 = the whole picture fits the window). `backend.ts` has the two custom shots (terminal replay to browser; three windows side by side).

Where a picture would show the random mock path (`/tmp/ref-mock-<hex>/workspace`), the camera looks away from it. Preview one frame of a film with `pnpm render <film> --lang en --still SEC`.
