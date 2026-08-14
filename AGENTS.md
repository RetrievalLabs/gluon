# AGENTS.md

## Project Overview

A Java modernization platform that migrates applications from Java 9 to Java 25 by updating deprecated/removed APIs, incompatible dependencies, and outdated versions while preserving existing behavior and minimizing unnecessary changes.

## Repository Structure

- app - contains different components of the platform
- docs - contains architecture and planning docs

## Coding Rules

- Create and review a plan before editing files.
- Make only required changes.
- Preserve existing behavior unless explicitly required otherwise.
- Update `docs/architecture.md` when architecture changes.

### Comments

- Add clear, concise comments for non-obvious logic.
- Prefer explaining **why** something is done rather than describing **what** the code does.

Good:
```rust
// Keep the original source location because diagnostics are mapped back to
// the user's code after the transformation is applied.
let location = source_map.resolve(node.span);
```

Avoid:

```rust
// Resolve the source location.
let location = source_map.resolve(node.span);
```


## Token-Efficient Communication

Use terse, technical communication by default.

### Core Rules

- Drop articles: `a`, `an`, `the`.
- Drop filler: `just`, `really`, `basically`, `actually`, `simply`.
- Drop pleasantries: no “sure”, “happy to”, “of course”.
- Drop hedging: no “maybe”, “it might be worth”, “consider” unless uncertainty matters.
- Use fragments when meaning stays clear.
- Use short words: `fix` instead of `implement solution`.
- State each fact once.
- Keep technical terms exact.
- Keep code, commands, paths, URLs, API names, and error strings exact.
- Never drop negation: `not`, `never`, `no`, `only`, `except`.
- Do not paste long raw logs unless asked. Quote shortest decisive line.
- Do not use decorative tables or emoji.
- Do not invent abbreviations like `cfg`, `impl`, `req`, `res`, `fn`.
- Do not use arrows as prose shorthand.
- Prefer pattern: `[thing] [action] [reason]. [next step].`

### Code Work

- Write code, comments, docs, commit messages, and PR text in normal professional prose.
- Keep user-facing explanations terse, but do not make persisted project files awkward or unclear.
- Preserve exact behavior and terminology over token savings.

### Example

Verbose:

> The issue is probably caused by the authentication middleware because it does not check whether the token has expired before allowing the request to
continue.

Preferred:

> Auth middleware bug. Token expiry not checked before request continues. Add expiry guard.


## Parallel Work

- Use multiple agents only for large tasks with independent work; use one agent for small or tightly coupled tasks.
- Use a read-only Context Agent to research relevant code, architecture, dependencies, patterns, and tests before implementation.
- Implementation agents use context findings and avoid duplicate research.
- Give each agent an isolated scope; avoid editing same files.
- Main agent reviews, integrates, resolves conflicts, and runs final validation.

### Example

- Context Agent: Research relevant context.
- Agent 1: Implement API changes.
- Agent 2: Implement parser changes.
- Agent 3: Add/update tests.
- Main Agent: Review, integrate, and validate.