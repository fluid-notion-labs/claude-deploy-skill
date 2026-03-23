# File Editing Primitives — Research

## Status
Complete.

## What Claude has natively in-container

Four tools, all available in every session:

| Tool | What it does | Limitations |
|------|-------------|-------------|
| `str_replace` | Replace unique string with new string | Fails on non-unique matches; requires exact whitespace; no line-number addressing |
| `create_file` | Write full file content | Full rewrite — expensive for large files, loses git blame granularity |
| `view` | Read file or line range | Read-only |
| `bash_tool` | Arbitrary shell commands | Full power — this is the escape hatch |

`str_replace` is the intended surgical tool but has a hard constraint: **the old string must appear exactly once in the file**. Duplicate function signatures, repeated patterns, or common boilerplate will cause it to refuse. Exact whitespace match is also required — tabs vs spaces, trailing spaces etc. will break it.

## What else is in the container

All available without install:

- **GNU sed 4.9** — insert/delete/replace by line number or pattern, in-place with `-i`
- **mawk** — awk for more complex line transformations
- **Perl 5.38** — one-liners with full regex; `-i` in-place
- **Python 3.12** — `re`, `fileinput`, `difflib`; most expressive for complex edits
- **GNU patch 2.7** — applies unified diff format; git-native
- **bash** — heredocs, process substitution

Not available: `ed`, `comby`, `fastmod`, `jq`, `rg`

## Edit formats in the wild

**LSP TextEdit** (Language Server Protocol):
```json
{ "range": { "start": {"line": 5, "character": 0}, "end": {"line": 7, "character": 0} }, "newText": "replacement\n" }
```
Line+character addressing. Used by VS Code, Neovim, Zed. Precise but verbose to construct.

**Zed** uses LSP-compatible TextEdit internally for its extension protocol. No separate format.

**Unified diff** (git-native):
```diff
--- a/file.txt
+++ b/file.txt
@@ -5,3 +5,4 @@
 context line
-removed line
+added line
+new line
 context line
```
Applied with `patch`. Most universal — git understands it, humans can read it, `python3 difflib` can generate it programmatically.

**str_replace_based_edit** (Anthropic's own tool format):
```json
{ "command": "str_replace", "path": "...", "old_str": "...", "new_str": "..." }
```
What Claude uses natively. Simple but constrained by uniqueness requirement.

## What exists on npm/pip

- **replace-in-file** (npm, MIT) — regex find/replace across files, CLI + API. Good for simple substitutions, not structural edits.
- **rope** (pip) — Python AST-aware refactoring. Overkill, language-specific.
- **comby** — structural search/replace. Not installed, would need install.

## Assessment: do we need a custom tool?

**Probably not.** The combination of:

1. `str_replace` for unique-string replacements (covers ~80% of cases)
2. `sed -i` for line-number or pattern-based insert/delete/replace
3. `python3` one-liners for complex multi-line or regex block operations
4. `patch` for applying pre-computed unified diffs

...covers every edit scenario without installing anything. The key insight is that `bash_tool` gives access to all of these — Claude should reach for `sed`/`python3` whenever `str_replace` won't work rather than trying to force it.

## Recommended patterns

```sh
# Insert line after pattern (even non-unique)
sed -i '/pattern/a\new line content' file.txt

# Insert line before pattern
sed -i '/pattern/i\new line content' file.txt

# Delete lines N through M
sed -i 'N,Md' file.txt

# Delete lines matching pattern
sed -i '/pattern/d' file.txt

# Replace Nth occurrence of pattern (not just first)
sed -i '0,/pattern/!{/pattern/s/old/new/}' file.txt

# Replace block between markers
python3 -c "
import re, sys
c = open('file.txt').read()
c = re.sub(r'(?<=START_MARKER\n).*?(?=END_MARKER)', 'new content\n', c, flags=re.DOTALL)
open('file.txt','w').write(c)
"

# Append to file
echo "new content" >> file.txt

# Apply a unified diff
patch file.txt < changes.patch
```

## Decision

No new tool needed. Claude should:
1. Use `str_replace` for unique-match replacements
2. Fall back to `sed -i` or `python3` via `bash_tool` for everything else
3. Never use `create_file` for edits to existing files unless the change touches >50% of the file

This should be documented in SKILL.md as part of the coding workflow.
