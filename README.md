# fabric-input

Controller discovery and the abstract pad for
[RaNor Fabric](https://github.com/ranson21/ranor-fabric) — the shared device
platform beneath RaNorTV and Apex.

This is the **first component to cross the Fabric boundary.** It was
`ranortv-input`, a crate inside `ranortv-os`, and it moved here with its
history intact rather than as a copy.

## Why this one first

`docs/platform-boundary.md` in the superproject sets three tests for Fabric
membership: no product-domain concepts in the API, more than one product
genuinely needs it, and — the one that is easy to miss — its shape is not
fitted to one product's interface.

This crate passes all three, and was the only component that did without
argument. It was written as a system crate from the start, for exactly this
reason: ADR-0006 decision 3 in `ranortv-os` made the input path
product-neutral before there was a second product to be neutral about. Its
`Intent` vocabulary is navigation, not media. Its `EventSource` boundary
already had a test double, which means the abstraction had been exercised as
an abstraction rather than assumed.

The crates alongside it — `bundle`, `store`, `pack` — are *probably* Fabric
and are deliberately still in `ranortv-os`. An interface with one caller has
not been tested as an interface, and Apex is the second caller that will
settle it.

## What it does

| Module | Role |
|---|---|
| `device.rs` | Discovery, connect and disconnect |
| `pad.rs` | The abstract pad: buttons and axes, per device, neutral on reconnect |
| `intent.rs` | Projection from pad state onto navigation intents |
| `source.rs` | The `EventSource` boundary — the crate does not own a thread |
| `synthetic.rs` | A test double implementing `EventSource` from a scripted list |
| `gilrs_backend.rs` | The real backend, behind the `gilrs-backend` feature |
| `error.rs` | Errors that cross the boundary |

**The crate does not own a thread.** Consumers have different timing needs — a
launcher has a UI event loop, an emulator a frame loop — and a thread here
would make one of them wrong.

## Features

`gilrs-backend` is **off by default**. `gilrs` pulls in `libudev-sys`, whose
build script probes pkg-config at configure time, so a default-on feature
would make every build require libudev headers. Consumers turn it on at their
call site, where the matching CI dependency is declared alongside it.

## Building

```sh
cargo test          # 20 tests, no system dependencies
```

The `gilrs-backend` feature needs libudev headers; nothing else does.

## Development

**Primary development happens in
[ranor-fabric](https://github.com/ranson21/ranor-fabric)**, at
`assets/fabric-input`, where it sits alongside the platform documents and the
RaNorTV OS that consumes it. It is also a submodule of
[apex](https://github.com/ranson21/apex), which consumes it as the second
product.

Its Terraform configuration lives in `ranor-fabric` and only there. Terragrunt
derives a composition's state prefix from its path relative to the root
`terragrunt.hcl`, so a matching directory in another superproject would be a
second owner of the same GitHub repository rather than a second configuration.

## Licence

Dual-licensed under Apache-2.0 or MIT, at your option.
