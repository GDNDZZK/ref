# Markdown tour

This reply uses most of what the reader has to render. Plain paragraphs wrap at the width of the window, and **bold**, *italic*, ~~strikethrough~~ and `inline code` can be mixed: **bold with `code` inside** and a [link](https://example.com/docs "Example docs").

## Lists

- A bullet
  - A nested bullet with a longer sentence that is long enough to wrap onto a second line in a narrow window
  - Another nested bullet
- Another bullet

1. First step
2. Second step
   1. A sub-step
   2. Another sub-step
3. Third step

- [x] A finished task
- [ ] An open task

## Quote

> A quote can span several lines and contain **formatting**.
>
> > And a quote inside a quote.

## Table

| Tool | Needs approval | Typical output |
|:-----|:--------------:|---------------:|
| `read` | no | numbered lines |
| `edit` | yes, outside `scratch/` | a diff |
| `bash` | depends on the command | text and an exit code |

## Code

Rust:

```rust
use std::collections::HashMap;

/// Counts words, ignoring case.
fn count_words(text: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for word in text.split_whitespace() {
        *counts.entry(word.to_lowercase()).or_insert(0) += 1;
    }
    counts
}
```

Python:

```python
def fib(n: int) -> int:
    a, b = 0, 1
    for _ in range(n):
        a, b = b, a + b
    return a
```

TypeScript:

```ts
type Part = { type: "text"; text: string } | { type: "tool"; name: string };

export function names(parts: Part[]): string[] {
  return parts.flatMap((p) => (p.type === "tool" ? [p.name] : []));
}
```

A shell session and a diff:

```bash
$ cargo test -p parser
running 12 tests
test result: ok. 12 passed; 0 failed
```

```diff
-    for i in 0..=items.len() {
+    for i in 0..items.len() {
```

A block without a language, and one with very long lines:

```
plain text in a block
```

```text
This line is deliberately very long so that the code block has to scroll sideways instead of wrapping: 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789
```

---

## Other scripts

中文和日本語のテキストも表示できます。Emoji 🙂 and accents: café, naïve, Zoë.

Horizontal rules, headings of every level and inline HTML such as <b>this</b> are shown as text, never as markup.

### Level three

#### Level four
