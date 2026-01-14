# Freenet Wiki

A decentralized wiki application built on [Freenet](https://freenet.org).

> **⚠️ Under Development**: This project is still in early development. APIs and features may change.

## Features

- **Markdown-based** wiki pages with `[[wiki links]]` syntax
- **Real-time collaboration** via revision + patches model
- **Cryptographic authentication** using Ed25519 signatures
- **Decentralized** - runs on the Freenet network without central servers

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Wiki Contract                           │
│  ┌───────────────┐  ┌──────────────┐  ┌─────────────────┐   │
│  │ Configuration │  │ Contributors │  │     Pages       │   │
│  │  (owner-only) │  │  (invites)   │  │ (revisions +    │   │
│  │               │  │              │  │  patches)       │   │
│  └───────────────┘  └──────────────┘  └─────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────┴───────────────────────────────┐
│                     Wiki Delegate                           │
│  • Stores signing keys locally                              │
│  • Signs patches before submission                          │
└─────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────┴───────────────────────────────┐
│                        Wiki UI                              │
│  • Markdown editor with preview                             │
│  • Page navigation with wiki links                          │
│  • Real-time updates via subscriptions                      │
└─────────────────────────────────────────────────────────────┘
```

## Project Structure

```
freenet-wiki/
├── common/                 # Shared types (state, pages, patches)
├── contracts/wiki-contract # Freenet contract implementation
├── delegates/wiki-delegate # Local key storage delegate
└── ui/                     # Dioxus web UI
```

## Building

Requires [cargo-make](https://github.com/sagiegurari/cargo-make):

```bash
cargo install cargo-make

# Build all crates
cargo make build

# Run tests
cargo make test

# Lint
cargo make clippy
```

## How It Works

### Revision + Patches Model

Each wiki page has:
- A **base revision** - signed full content snapshot
- **Pending patches** - signed line-based operations

Patches use content-addressed operations (targeting lines by hash, not position) for commutative merging. Any contributor can **commit** patches to create a new revision.

### Contributors

The wiki owner can invite contributors, who can then invite others, creating a chain of trust. All edits must be signed by an authorized contributor.

## License

LGPL-3.0
