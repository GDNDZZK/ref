# A long article about streaming conversations

## 1. Why streaming needs care

When a reply arrives token by token, the reader sees the text grow while the layout changes under it. A good reader keeps the line the user is looking at in place, appends new lines below it, and follows the end of the text only while the user has not scrolled away.

When a reply arrives token by token, a reader sees the text grow while the layout changes under it. A good reader keeps the line the user is looking at in place, appends new lines below it, and follows the end of the text only while the user has not scrolled away. Details differ between interfaces, but the idea stays the same: the reader must stay calm while the content is busy.

```text
turn := prompt -> (reasoning | text | tool)* -> idle
```

- Point one of section 1
- Point two, with `inline code`
- Point three, with a longer sentence that wraps when the window is narrow

## 2. Measuring a turn

A turn starts when the prompt is sent and ends when the model has nothing more to say and no tool is left to run. Everything in between belongs to one block of the conversation: reasoning, text, tool calls and their results.

A turn starts when a prompt is sent and ends when the model has nothing more to say and no tool is left to run. Everything in between belongs to one block of the conversation: reasoning, text, tool calls and their results. Details differ between interfaces, but the idea stays the same: the reader must stay calm while the content is busy.

```rust
enum Outcome {
    Done,
    Aborted,
    Failed(String),
}

fn finish(outcome: Outcome) -> bool {
    matches!(outcome, Outcome::Done)
}
```

- Point one of section 2
- Point two, with `inline code`
- Point three, with a longer sentence that wraps when the window is narrow

## 3. Tool calls in flight

A tool call passes through several states. Its arguments stream first, then the call may wait for approval or for a delay to pass, then it runs, and finally it ends as done, failed, denied or cancelled.

A tool call passes through several states. Its arguments stream first, an the call may wait for approval or for a delay to pass, then it runs, and finally it ends as done, failed, denied or cancelled. Details differ between interfaces, but the idea stays the same: the reader must stay calm while the content is busy.

```ts
type ToolState =
  | "streaming"
  | "awaiting_permission"
  | "delayed"
  | "running"
  | "done"
  | "error"
  | "denied"
  | "cancelled";
```

- Point one of section 3
- Point two, with `inline code`
- Point three, with a longer sentence that wraps when the window is narrow

## 4. Long outputs

Commands can print thousands of lines. The backend keeps the head and the tail and elides the middle, and the reader shows the result in a block that can be collapsed so it does not push the rest of the conversation out of view.

Commands can print thousands of lines. The backend keeps a head and the tail and elides the middle, and the reader shows the result in a block that can be collapsed so it does not push the rest of the conversation out of view. Details differ between interfaces, but the idea stays the same: the reader must stay calm while the content is busy.

```bash
seq 1 5000 | tail -n 3
grep -rn "TODO" src | head -20
```

- Point one of section 4
- Point two, with `inline code`
- Point three, with a longer sentence that wraps when the window is narrow

## 5. Retrying

When the provider is overloaded the request is retried with growing waits. While it waits, the status line shows the time left and the reason; only a final failure becomes an error block in the conversation.

When a provider is overloaded the request is retried with growing waits. While it waits, the status line shows the time left and the reason; only a final failure becomes an error block in the conversation. Details differ between interfaces, but the idea stays the same: the reader must stay calm while the content is busy.

```python
def backoff(attempt: int, base: float = 2.0, cap: float = 600.0) -> float:
    return min(cap, base * 2 ** (attempt - 1))
```

- Point one of section 5
- Point two, with `inline code`
- Point three, with a longer sentence that wraps when the window is narrow

## 6. Compaction

When the context fills up, the older part of the conversation is replaced by a summary for the model. The user keeps seeing the full transcript, with the summary shown as a collapsed block at the point where it took effect.

When a context fills up, the older part of the conversation is replaced by a summary for the model. The user keeps seeing the full transcript, with the summary shown as a collapsed block at the point where it took effect. Details differ between interfaces, but the idea stays the same: the reader must stay calm while the content is busy.

```text
turn := prompt -> (reasoning | text | tool)* -> idle
```

- Point one of section 6
- Point two, with `inline code`
- Point three, with a longer sentence that wraps when the window is narrow

## 7. Why streaming needs care

When a reply arrives token by token, the reader sees the text grow while the layout changes under it. A good reader keeps the line the user is looking at in place, appends new lines below it, and follows the end of the text only while the user has not scrolled away.

When a reply arrives token by token, a reader sees the text grow while the layout changes under it. A good reader keeps the line the user is looking at in place, appends new lines below it, and follows the end of the text only while the user has not scrolled away. Details differ between interfaces, but the idea stays the same: the reader must stay calm while the content is busy.

```text
turn := prompt -> (reasoning | text | tool)* -> idle
```

- Point one of section 7
- Point two, with `inline code`
- Point three, with a longer sentence that wraps when the window is narrow

## 8. Measuring a turn

A turn starts when the prompt is sent and ends when the model has nothing more to say and no tool is left to run. Everything in between belongs to one block of the conversation: reasoning, text, tool calls and their results.

A turn starts when a prompt is sent and ends when the model has nothing more to say and no tool is left to run. Everything in between belongs to one block of the conversation: reasoning, text, tool calls and their results. Details differ between interfaces, but the idea stays the same: the reader must stay calm while the content is busy.

```rust
enum Outcome {
    Done,
    Aborted,
    Failed(String),
}

fn finish(outcome: Outcome) -> bool {
    matches!(outcome, Outcome::Done)
}
```

- Point one of section 8
- Point two, with `inline code`
- Point three, with a longer sentence that wraps when the window is narrow

## 9. Tool calls in flight

A tool call passes through several states. Its arguments stream first, then the call may wait for approval or for a delay to pass, then it runs, and finally it ends as done, failed, denied or cancelled.

A tool call passes through several states. Its arguments stream first, an the call may wait for approval or for a delay to pass, then it runs, and finally it ends as done, failed, denied or cancelled. Details differ between interfaces, but the idea stays the same: the reader must stay calm while the content is busy.

```ts
type ToolState =
  | "streaming"
  | "awaiting_permission"
  | "delayed"
  | "running"
  | "done"
  | "error"
  | "denied"
  | "cancelled";
```

- Point one of section 9
- Point two, with `inline code`
- Point three, with a longer sentence that wraps when the window is narrow

## 10. Long outputs

Commands can print thousands of lines. The backend keeps the head and the tail and elides the middle, and the reader shows the result in a block that can be collapsed so it does not push the rest of the conversation out of view.

Commands can print thousands of lines. The backend keeps a head and the tail and elides the middle, and the reader shows the result in a block that can be collapsed so it does not push the rest of the conversation out of view. Details differ between interfaces, but the idea stays the same: the reader must stay calm while the content is busy.

```bash
seq 1 5000 | tail -n 3
grep -rn "TODO" src | head -20
```

- Point one of section 10
- Point two, with `inline code`
- Point three, with a longer sentence that wraps when the window is narrow

## 11. Retrying

When the provider is overloaded the request is retried with growing waits. While it waits, the status line shows the time left and the reason; only a final failure becomes an error block in the conversation.

When a provider is overloaded the request is retried with growing waits. While it waits, the status line shows the time left and the reason; only a final failure becomes an error block in the conversation. Details differ between interfaces, but the idea stays the same: the reader must stay calm while the content is busy.

```python
def backoff(attempt: int, base: float = 2.0, cap: float = 600.0) -> float:
    return min(cap, base * 2 ** (attempt - 1))
```

- Point one of section 11
- Point two, with `inline code`
- Point three, with a longer sentence that wraps when the window is narrow

## 12. Compaction

When the context fills up, the older part of the conversation is replaced by a summary for the model. The user keeps seeing the full transcript, with the summary shown as a collapsed block at the point where it took effect.

When a context fills up, the older part of the conversation is replaced by a summary for the model. The user keeps seeing the full transcript, with the summary shown as a collapsed block at the point where it took effect. Details differ between interfaces, but the idea stays the same: the reader must stay calm while the content is busy.

```text
turn := prompt -> (reasoning | text | tool)* -> idle
```

- Point one of section 12
- Point two, with `inline code`
- Point three, with a longer sentence that wraps when the window is narrow

## Summary

Streaming text is easy to get roughly right and hard to get smooth. Watch the frames, not just the final result.
