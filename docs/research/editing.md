# File Editing Primitives — Research

## Status
Stub — to be filled in.

## Questions to answer

1. What file-editing capabilities does Claude have natively in-container?
2. What edit formats exist in the wild (Zed, LSP, unified diff, etc.)? Which are a good fit?
3. Is a small installable node/python CLI tool the right approach?
4. What operations are needed: insert-line, append, remove-line, find-replace, insert-at-pattern?
5. What already exists that could fill this role?

## Context

Claude currently edits files by rewriting full content via `create_file` or `str_replace` (unique string match → replacement). `str_replace` is fragile on non-unique strings and requires exact whitespace matches. Large files are expensive to rewrite in full.

Goal: reliable surgical edits without full-file rewrites, installable into any container session.

## Research notes

_(to be filled)_

## Candidates

_(to be filled)_

## Decision

_(to be filled)_
