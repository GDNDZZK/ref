# qutie design

> **Historical (qutie).** ref-gui no longer talks to LLM providers itself: the `llm` and `infra` layers described here were replaced by `proto`, `client` and `session`, and the program is a client of ref-server. See ARCHITECTURE.md for the current design; the visual and motion principles below still apply.

Cross-platform GUI coding agent. C++23, Qt Quick (QML), CMake + Ninja, GPL-3.0-or-later.

## Decisions

- Dependencies come from system packages (no vcpkg). Qt floor 6.10 (required by KSyntaxHighlighting).
- Libraries: QCoro, KSyntaxHighlighting (QML `SyntaxHighlighter`), QtKeychain, dtl. Markdown via Qt's md4c, grep via external `rg`.
  A library is only `find_package`d once code uses it.
- Config: `~/.config/qutie/config.jsonc`, read-only. JSONC is lowered to JSON in place (comments and trailing commas
  become spaces, offsets preserved) and parsed by `QJsonDocument`. M1 shape: `{"endpoint": {"baseUrl", "model",
  "apiKey"?}}`. The API key is plain text in the file for now (user's call); QtKeychain comes with a settings UI.
  `max_tokens` is not sent yet; it will become a config field.
- First provider: OpenAI-compatible Chat Completions (streaming, tool calls, `reasoning_content` / `reasoning`).
  Reasoning is sent back in the field it arrived in, since replies already span several user turns.
- History matches the screen: a reply that failed or was stopped stays in the history as far as it got.
  Failures show as an `error` block (rare, must be noticed): its badge pops in with a spring and ring, and red
  tint spreads out of the badge as a hard-edged circle at constant speed (the card enters neutral). No shake: it was the only sideways, "gestural" motion in the app.
  An error arriving while the user reads further up puts a "!" badge on the jump-to-latest button.
- Streamed text is paced (`StreamPacer`): bursts are released at a steady rate, so each runs out as the next
  is due. Shown text trails the stream by at most 200 ms; stopping shows the rest at once.
- Front end first: build the UI against a mock driver, then plug in the API back end. `--mock` keeps the
  mock for UI work; it can go once the back end is settled.
- Cancellation: `std::stop_token`. Stopping completes the request before `request_stop()` returns, so an
  owner can stop from its destructor. Each ChatSession owns its driver, client and network manager (no
  singletons), so several sessions (tabs) can coexist later.

## Layers

```
app (QML + ChatSession)  ->  ui (ConversationModel, FenceSplitter, Driver: MockDriver | AgentDriver)
                             AgentDriver -> llm (config, OpenAiClient, openai_wire) -> infra (http_stream, sse, jsonc)
                             M2 puts an agent loop between AgentDriver and llm.
```

## UI

- Model rows are blocks, not messages: user, reasoning, markdown, code, tool. Code fences are split out by
  `FenceSplitter` (CommonMark rules) so each code block gets its own highlighted TextEdit.
  Rows are only appended or updated (no resets), so view transitions work.
- Visuals: light and dark, follows the system until the user toggles. Everything left-aligned; only user
  messages get a rounded bubble.
- New blocks fade in and drop `Motion.enterDrop` px into place (from above). User tuned it to 48.
- Window frame: native, untouched.
- Motion (all timing in `Motion.qml`): exponential ease-out (`OutExpo`) almost everywhere, no overshoot in
  frequent interactions. Overshoot only for rare moments (theme toggle, `OutBack`) or for self-contained
  flourishes that move nothing else (tool completion: elastic spring plus a ring).
- Theme switch: snapshot crossfade (grab the page, swap colors, fade the snapshot out).
- Turns: each user message after the first opens with a full-width divider; the bubble is accent-tinted.
- Auto-follow: only user gestures (movementStarted/Ended, scroll bar drag) change `following`; never compare
  contentY against contentHeight, which ListView only estimates. Following chases the end with a per-frame
  exponential ease (`Motion.followLag`), not jumps.
- Continuous targets (streaming block height, scroll follow) use a per-frame exponential chase via
  FrameAnimation. `Behavior` only for discrete targets: it restarts on every target change and stalls
  when the target itself animates.
- Foldable blocks (reasoning, tool) keep their header pinned to the view's top while expanded, until the
  block's end pushes it out. Folding from a pinned header scrolls back with the fold, so the header stays put.
- Hold: during a turn the end never moves up; room the content gives back is kept as footer space and
  released with an animation when the turn ends. Prevents bouncing when wrapped lines unwrap.

## Progress

- [x] M0 skeleton, JSONC, SSE parser (with tests)
- [x] UI v1 against MockDriver: blocks, streaming, code highlighting, tool cards, reasoning fold, theme toggle
- [x] UI v1 iterated by hand: turn dividers, entry drop, smooth follow, hold, per-frame height chase,
      slow/long mock scripts. Phase closed; user reviewing code.
- [x] Review round 1: drawn chevron icons, themed scroll bar, exact geometry (all rows kept), wheel
      chase (Wayland reports wheels as touchpad), entrance as transform, whole-pixel scroll chase.
- [ ] M1 http_stream + OpenAiClient + AgentDriver + error block (builds clean with GCC 16 and Clang 23 on
      Qt 6.12; tests pass, also under ASan+UBSan). First real-server run done by the user; fixes after it:
      StreamPacer, new error animation, error badge on jump-to-latest (awaiting the user's visual check)

See `HANDOFF.md` (agent context) and `ARCHITECTURE.md` (human-oriented walkthrough).
