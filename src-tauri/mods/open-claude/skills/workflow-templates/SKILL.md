---
name: workflow-templates
description: Create a reusable Claude Code workflow and save it as an Open Claude template. Use when the user says "erstell einen Workflow … als Vorlage", "save as workflow template", "Workflow-Vorlage". Does not run it.
---

# Workflow templates

A template is a Workflow-tool script saved where Claude Code loads named workflows. Open Claude lists these files in its "Workflows" tab, where they can be edited (builder templates), started and deleted.

## 1. Where

- Default (all projects): `${CLAUDE_CONFIG_DIR:-~/.claude}/workflows/<name>.js`
- Only when the user means this project: `.claude/workflows/<name>.js` in the project root (overrides a global one with the same name)
- `<name>` is kebab-case (`^[a-z0-9][a-z0-9-]*$`) and identical to `meta.name`.
- Check first that the file does not exist yet; if it does, ask before overwriting or pick another name.

## 2. Builder-compatible template (preferred)

Use this whenever the flow fits: linear phases, each phase runs its agents either `parallel` (all at once, all get the previous phase's result) or `sequence` (one after another, each gets the previous agent's result). The app can then edit it in its form.

Write the file **exactly** in this shape — it is generator output, byte for byte. Only the data changes. The last line repeats all data as JSON and must match the code above it:

```js
export const meta = {
  name: "code-review",
  description: "Review the current diff from several angles and write one report",
  whenToUse: "Before committing a larger change",
  phases: [
    { title: "Find" },
    { title: "Report" },
  ],
}

// {{args}} = Aufruf-Argumente, {{prev}} = Ergebnis des vorigen Schritts
const str = (v) => (v == null ? "" : typeof v === "string" ? v : JSON.stringify(v, null, 2))
const fill = (p, prev) => p.replace(/\{\{(args|prev)\}\}/g, (_, k) => str(k === "args" ? (typeof args === "undefined" ? null : args) : prev))
let prev = null

phase("Find")
prev = (await parallel([
  () => agent(fill("Review the uncommitted diff (git diff HEAD) for correctness bugs. Focus: {{args}}", prev), {"label":"Correctness","phase":"Find","model":"sonnet","effort":"high","schema":{"type":"object","properties":{"findings":{"type":"array","items":{"type":"string"}}},"required":["findings"]}}),
  () => agent(fill("Review the uncommitted diff (git diff HEAD) for security issues. Focus: {{args}}", prev), {"label":"Security","phase":"Find"}),
])).filter(Boolean)

phase("Report")
prev = (await agent(fill("Merge these review results into one short report, most severe first:\n{{prev}}", prev), {"label":"Report","phase":"Report","model":"haiku","effort":"low"})) ?? prev
return prev

// open-claude-builder: {"v":1,"name":"code-review","description":"Review the current diff from several angles and write one report","whenToUse":"Before committing a larger change","phases":[{"title":"Find","mode":"parallel","agents":[{"label":"Correctness","prompt":"Review the uncommitted diff (git diff HEAD) for correctness bugs. Focus: {{args}}","model":"sonnet","effort":"high","schema":"{\"type\":\"object\",\"properties\":{\"findings\":{\"type\":\"array\",\"items\":{\"type\":\"string\"}}},\"required\":[\"findings\"]}"},{"label":"Security","prompt":"Review the uncommitted diff (git diff HEAD) for security issues. Focus: {{args}}","model":"","effort":"","schema":""}]},{"title":"Report","mode":"sequence","agents":[{"label":"Report","prompt":"Merge these review results into one short report, most severe first:\n{{prev}}","model":"haiku","effort":"low","schema":""}]}]}
```

Generation rules (the app regenerates the file from the last line, so follow them literally):

- `meta`: `name`, `description`, optional `whenToUse` (line omitted when empty), then one `    { title: "…" },` line per phase. Strings are `JSON.stringify` output; the closing line is exactly `}`.
- Per phase: `phase("<title>")`, then for `parallel` the `prev = (await parallel([ … ])).filter(Boolean)` block with one `  () => agent(…),` line per agent; for `sequence` one `prev = (await agent(…)) ?? prev` line per agent.
- Agent call: `agent(fill(<prompt as JSON string>, prev), <options as compact JSON>)`. Options in this order, empty ones left out: `label`, `phase` (always the phase title), `model`, `effort`, `schema` (as an object).
- Last line: `// open-claude-builder: ` + compact JSON `{"v":1,"name","description","whenToUse","phases":[{"title","mode","agents":[{"label","prompt","model","effort","schema"}]}]}`. All fields present; empty ones are `""`; `schema` is the schema as a JSON **string**. Escape U+2028/U+2029 as `\u2028`/`\u2029`.

| Field | Values |
|---|---|
| `mode` | `"parallel"` or `"sequence"` |
| `model` | `""` (inherit session model, the default), `"sonnet"`, `"opus"`, `"haiku"`, `"fable"` |
| `effort` | `""` (inherit), `"low"`, `"medium"`, `"high"`, `"xhigh"`, `"max"` |
| `schema` | `""` for free text, or a JSON object `{"type":"object","properties":{…},"required":[…]}`; `required` only names keys from `properties` |
| `label` | short display name, may be `""` |
| phase `title` | non-empty, unique, no `"` or `\` |

Placeholders in prompts, replaced at run time:
- `{{args}}` — the workflow's call arguments (a string verbatim, anything else as JSON)
- `{{prev}}` — the previous step's result: in `sequence` the previous agent's, in `parallel` the previous phase's (an array without `null`s)

## 3. Otherwise: free script

If the flow needs loops, votes, `pipeline()`, worktree isolation, custom agent types or conditionals, write a normal workflow script following the `workflow-authoring` skill, **without** the `// open-claude-builder:` line. The app can then only start and delete it.

## 4. Rules for both

- `export const meta = {…}` is the first statement and a pure literal; phase titles in `meta.phases` match the `phase()` calls exactly.
- Plain JavaScript: no TypeScript, no `import`, no `Date.now()`/`new Date()`/`Math.random()`.

## 5. Afterwards

Do **not** run it. Tell the user the file path and that the template appears in Open Claude's "Workflows" tab (start with ▶, edit with ✎ if builder-compatible), or can be started in a new session with `/<name>`.
