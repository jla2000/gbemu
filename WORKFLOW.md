# Working rules

How tasks in `SPEC.md` get executed. Solo project, jj (colocated with git).

## Task source

- `SPEC.md` milestone checkboxes (`M0`–`M7`) are the task list. Work in
  milestone order, top to bottom within a milestone.
- One task in flight at a time. If a checkbox turns out too big once
  started, split it into smaller checkboxes in `SPEC.md` first, then
  continue — don't silently scope-creep.
- Mirror the current task into `TodoWrite` at the start of a work session so
  progress is visible; keep exactly one `in_progress`.

## VCS: jj (colocated with git)

No branches, no staging area. Linear working-copy changes on top of `@`.

Per task:
1. `jj new` (if not already on a fresh empty change) to start the task.
2. Do the work.
3. Run checks (see Definition of Done).
4. `jj describe -m "<message>"` to describe the completed change.
5. `jj new` to start the next task's change.

Commit message format: `M<N>: <imperative summary>`
Examples:
- `M0: scaffold cargo workspace with gb-core and gb-tui crates`
- `M1: implement SM83 register set and flags`
- `M2: pass dmg-acid2 and mealybug ppu tests`

Never rewrite/abandon an already-described change without the user asking
for it explicitly.

## Definition of done

A checkbox may be ticked only when, in the same change:
- `cargo build --workspace` and `cargo check --workspace` succeed.
- Relevant tests pass:
  - Unit tests always (`cargo test -p gb-core`).
  - Blargg / Mealybug / dmg-acid2 harness tests when the task claims to
    satisfy them (per `SPEC.md` milestone text).
- The corresponding `SPEC.md` checkbox is ticked.

## Test failures / missing test ROMs

Blargg/Mealybug/acid2 ROMs are not redistributed and must be fetched by the
user into `roms/`. If a task's tests fail, or required ROMs are missing:

- Stop. Do not mark the checkbox done and do not move to the next task.
- Report the blocker plainly (what's missing, what failed, where).
- Propose a fix or workaround, or ask the user to supply the missing file.
- Wait for the user before proceeding.

## SPEC.md edits during implementation

- Checkboxes: tick freely as tasks complete.
- Spec text: may be amended when implementation reveals the plan was wrong
  or incomplete (e.g. a component needs splitting, an approach doesn't
  work). Call out any such change explicitly in the summary to the user —
  don't edit silently.
