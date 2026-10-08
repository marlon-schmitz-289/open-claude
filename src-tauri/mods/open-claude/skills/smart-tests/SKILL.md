---
name: smart-tests
description: Run only the tests that matter for the current change instead of the whole suite. Use whenever you are about to run tests to verify a change (after editing code, fixing a bug, refactoring), or the user says "run the tests", "test this", "check it still works". Picks the affected test files/targets from the diff; the full suite only when the change is broad or before a commit/release.
---

# Smart tests

Goal: fast, meaningful feedback. Run the smallest set of tests that can fail because of this change, then widen only when there is a reason.

## 1. What changed

```sh
git diff --name-only HEAD          # uncommitted changes
git diff --name-only @{u}..HEAD    # plus unpushed commits, if verifying a branch
```

If the change is only docs, comments, formatting or config without runtime effect: no tests, say so.

## 2. Map changes to tests

- **Changed test file** → run that file.
- **Changed source file** → its test file(s): same name with `.test`/`.spec`/`_test`/`Tests`, a sibling `tests/` or `__tests__/` folder, or tests that import it (`grep -rl "<module name>" <test dirs>`).
- **Shared code** (utils, types, config, build files, lockfiles, test setup, fixtures, CI) → widen to the whole package/crate/project that uses it.
- **No matching tests** → do not run the full suite just to "run something". Say that no test covers the change and suggest one if the logic is non-trivial.

## 3. Run targeted

| Stack | Targeted run |
|---|---|
| vitest | `npx vitest run <files>` or `npx vitest related <src files> --run` |
| jest | `npx jest --findRelatedTests <src files>` or `npx jest <files>` |
| node:test | `node --test <files>` |
| cargo | `cargo test -p <crate>` and/or `cargo test <module_or_test_name>`; `--lib` if only the library changed |
| dotnet | `dotnet test <project> --filter "FullyQualifiedName~<Class>"` |
| pytest | `pytest <file>::<test>` or `pytest <file>`; `-x` to stop at the first failure |
| go | `go test ./<pkg>/... -run <Name>` |

Prefer the project's own scripts (`package.json`, `Makefile`, `justfile`) when they accept a filter.

## 4. When to run everything

- The user asks for the full suite.
- Before a commit, push or release, if the project expects it.
- The change touches shared infrastructure (see above) or many packages.
- Targeted tests pass but you have a concrete reason to suspect side effects elsewhere.

## 5. Report

One line per run: which tests ran and why that selection, the result, and anything you deliberately did not run.
