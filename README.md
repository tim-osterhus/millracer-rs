# Millracer

Millracer is the Rust implementation target for the Python
[`millracer`](https://github.com/tim-osterhus/millracer) operator harness.

This first Rust release is intentionally small: it claims the public crate name
and provides a stable command shell while the private Millrace auto-port harness
builds parity with the Python reference.

## Install

```bash
cargo install millracer
```

## Status

The Python implementation is the reference source while the Rust crate is being
ported. Public crate releases are produced from this repository; local
Millrace daemon state used for autonomous porting lives under ignored
`millrace-agents/` workspace files.
