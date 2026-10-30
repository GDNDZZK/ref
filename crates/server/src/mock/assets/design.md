# Design

The crate has one module, `parser`. It takes a list of `Item` values and returns
one `Out` row per item, with an upper-cased label and a doubled weight.

## Open questions

1. Should empty names be rejected?
2. Should `total` saturate instead of overflowing?
