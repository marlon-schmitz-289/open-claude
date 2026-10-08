import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register, RenderElement, ToolResultOf } from 'claude-code'

import type { Limit, Panel } from '../types'

const PANE = { id: 'session', title: 'Session', columns: 36 } as const
const DAYS = ['So', 'Mo', 'Di', 'Mi', 'Do', 'Fr', 'Sa']
const HOUR = 3_600_000
const WINDOWS: Record<string, { label: string; ms: number }> = {
  five_hour: { label: 'SESSION 5h', ms: 5 * HOUR },
  seven_day: { label: 'WOCHE 7d', ms: 7 * 24 * HOUR },
}

const fresh = (startedAt: number): Panel => ({
  startedAt,
  context: { tokens: null, window: 0, percent: null },
  compactLeft: null,
  limits: [],
  cost: null,
  model: '',
  effort: null,
  tokens: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  turns: 0,
  active: [],
  todos: [],
})
const panel = atom({ plugin: 'open-claude', key: 'panel' } as const, fresh(0))

const pad = (n: number) => String(n).padStart(2, '0')

// Duenne Linie wie die Rahmen der TUI; Halbzelle per ╸. Gefuellt und leer getrennt, weil verschiedene Farben.
export const bar = (frac: number, width: number): [string, string] => {
  const halves = Math.round(Math.min(1, Math.max(0, frac)) * width * 2)
  const fill = '━'.repeat(Math.floor(halves / 2)) + (halves % 2 ? '╸' : '')
  return [fill, '─'.repeat(width - fill.length)]
}

// Prognose in % bei Reset (ungedeckelt, > 100 = zu schnell); undefined, solange das Fenster nicht laeuft.
export const pace = (used: number, resetsAt: string | undefined, ms: number, now: number) => {
  const reset = resetsAt ? Date.parse(resetsAt) : NaN
  const frac = (now - (reset - ms)) / ms
  return Number.isNaN(frac) || frac <= 0 ? undefined : used / frac
}

export const level = (percent: number) => (percent < 50 ? 'claude' : percent < 80 ? 'warning' : 'error')

export const fmtTokens = (n: number) =>
  n < 1000 ? String(n) : n < 1e6 ? `${Math.round(n / 1000)}k` : `${(n / 1e6).toFixed(1)}M`

export const fmtDuration = (ms: number) => {
  const m = Math.max(0, Math.floor(ms / 60_000))
  return m < 60 ? `${m}m` : `${Math.floor(m / 60)}h ${m % 60}m`
}

// Innerhalb eines Tages Uhrzeit plus Restzeit, sonst Wochentag.
export const fmtReset = (iso: string | undefined, now: number) => {
  const at = iso ? new Date(iso) : undefined
  if (!at || Number.isNaN(at.getTime())) return undefined
  const clock = `${pad(at.getHours())}:${pad(at.getMinutes())}`
  const left = at.getTime() - now
  return left < 24 * HOUR ? `Reset ${clock} · noch ${fmtDuration(left)}` : `Reset ${DAYS[at.getDay()]} ${clock}`
}

const middle = (s: string, n: number) => (s.length <= n ? s : `${s.slice(0, Math.ceil((n - 1) / 2))}…${s.slice(s.length - Math.floor((n - 1) / 2))}`)

const pct = (p: number) => `${Math.round(p)}%`

// Kompakte Zeile fuers Inline-Pane.
export const line = (s: Panel) => {
  const parts: string[] = []
  if (s.context.percent !== null) parts.push(`Ctx ${Math.round(s.context.percent)}%`)
  for (const l of s.limits) if (l.kind in WINDOWS) parts.push(`${l.kind === 'five_hour' ? '5h' : '7d'} ${Math.round(l.percentUsed)}%`)
  if (s.cost) parts.push(`$${s.cost.toFixed(2)}`)
  return parts.length ? parts.join(' · ') : undefined
}

const limitsOf = (limits: readonly Limit[]): Limit[] => limits.map(l => ({ ...l }))

// App-Panel liest die Datei; ohne Env (claude ausserhalb der App) nur Atom. Pfad wird in session.start
// gemerkt und aus der Env genommen, damit verschachtelte `claude -p` nicht dieselbe Datei ueberschreiben.
// ponytail: Hot-Reload der Mod verliert den Pfad (Env schon leer) – bis zum naechsten claude-Start keine Writes.
let file: string | undefined
// Writes laufen nacheinander und schreiben den aktuellen Atom-Stand, sonst ueberholt ein aelterer Snapshot.
let chain = Promise.resolve()
// Fehler duerfen weder Tool-Calls noch die Kette kippen.
const put = ($: EngineInterface, f: (s: Panel) => Panel) =>
  update($, panel, f)
    .then(async () => {
      file ??= await $.env.get('OPEN_CLAUDE_PANEL')
      if (file) chain = chain.then(async () => $.fs.write(file!, JSON.stringify(await read($, panel)))).catch(() => {})
      return chain
    })
    .catch(() => {})

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    file ??= await $.env.get('OPEN_CLAUDE_PANEL')
    await $.env.set('OPEN_CLAUDE_PANEL', undefined)
    await $.command.register({ name: 'panel', description: 'Session-Panel ein/aus' })
    const done = await next(e)
    const [usage, model] = await Promise.all([$.session.usage(), $.session.model()])
    await put($, s => ({
      ...(s.startedAt === usage.startedAt ? s : fresh(usage.startedAt)),
      context: { tokens: usage.context.tokens ?? null, window: usage.context.window, percent: usage.context.percent ?? null },
      limits: limitsOf(usage.rateLimits),
      cost: usage.cost?.usd ?? null,
      model,
    }))
    // Countdowns und Dauer laufen ohne neue Daten weiter.
    $.clock.every(30_000, () => $.ui.invalidate('ui.render'))
    if (!(await $.store.get('themeHint'))) {
      $.ui.toast('Tipp: /theme → Open Claude (Hell/Dunkel, folgt der App)')
      await $.store.set('themeHint', true)
    }
    return done
  })

  on('command.run', { command: 'panel' }, async $ => {
    if ((await $.ui.panes()).some(p => p.id === PANE.id)) {
      await $.ui.close({ id: PANE.id })
      return { text: 'Session-Panel geschlossen.' }
    }
    await $.ui.open(PANE)
    return { text: 'Session-Panel geöffnet.' }
  })

  on('session.measure', async ($, e, next) => {
    await put($, s => ({
      ...s,
      context: { tokens: e.context.tokens ?? null, window: e.context.window, percent: e.context.percent ?? null },
      limits: limitsOf(e.rateLimits),
      cost: e.cost?.usd ?? s.cost,
    }))
    return next(e)
  })

  on('session.end', async ($, e, next) => {
    if (e.reason === 'clear') {
      const now = await $.clock.now()
      await put($, s => ({ ...fresh(now), limits: s.limits, model: s.model, effort: s.effort }))
    }
    return next(e)
  })

  on('turn.step', async function* ($, e, next) {
    const effort = e.effort === undefined ? null : String(e.effort)
    if (!e.agentId) {
      const s = await read($, panel)
      if (s.model !== e.model || s.effort !== effort) await put($, s => ({ ...s, model: e.model, effort }))
    }
    return yield* next(e)
  })

  on('turn.complete', async ($, e, next) => {
    const u = e.usage
    const summary = e.agentId ? undefined : (await $.session.usage({ breakdown: 'summary' })).context.breakdown
    const end = e.reason === 'answer' ? ('ok' as const) : ('err' as const)
    await put($, s => ({
      ...s,
      turns: s.turns + (e.agentId ? 0 : 1),
      // Abgebrochene Tool-Calls kommen nie aus 'run' zurueck; Turn-Ende raeumt sie auf.
      active: e.agentId ? s.active : s.active.map(a => (a.state === 'run' ? { ...a, state: end } : a)),
      tokens: u
        ? {
            input: s.tokens.input + u.input_tokens,
            output: s.tokens.output + u.output_tokens,
            cacheRead: s.tokens.cacheRead + u.cache_read_input_tokens,
            cacheWrite: s.tokens.cacheWrite + u.cache_creation_input_tokens,
          }
        : s.tokens,
      compactLeft: !summary
        ? s.compactLeft
        : summary.isAutoCompactEnabled && summary.autoCompactThreshold !== undefined
          ? summary.autoCompactThreshold - summary.totalTokens
          : null,
    }))
    return next(e)
  })

  on('tool.call', async ($, e, next) => {
    const input = e as unknown as Record<string, unknown>
    const target = [input.file_path, input.pattern, input.command, input.description].find(v => typeof v === 'string')
    const id = e.tool_use_id ?? String(await $.clock.now())
    const entry = { id, tool: String(e.tool), target: typeof target === 'string' ? target : '', state: 'run' as const }
    await put($, s => ({ ...s, active: [...s.active, entry].slice(-4) }))
    const ran = await next(e)
    const state = ran.isError || 'deny' in ran ? ('err' as const) : ('ok' as const)
    await put($, s => ({ ...s, active: s.active.map(a => (a.id === id ? { ...a, state } : a)) }))
    return ran
  })

  on('tool.call', { tool: 'TodoWrite' }, async ($, e, next) => {
    const ran = await next(e)
    const todos = (ran.result as ToolResultOf<'TodoWrite'> | undefined)?.newTodos
    if (todos) await put($, s => ({ ...s, todos: todos.map(t => ({ content: t.content, status: t.status })) }))
    return ran
  })

  on('ui.render', { component: 'Pane', requestId: 'session' }, async ($, e) => {
    const { Box, Text } = $.ui.resolve(e)
    const s = await read($, panel)
    const now = await $.clock.now()
    const { bodyColumns: cols, placement, scroll } = e.props

    if (placement === 'inline') return <Text color="inactive">{line(s) ?? 'Noch keine Daten'}</Text>

    // Rahmen und paddingX je 2 Spalten.
    // ponytail: Flachmodus 2 Spalten schmaler als noetig, eigene Breite falls es stoert
    const width = Math.max(1, cols - 4)
    const gauge = (label: string, p: number, extra: (string | undefined)[], fast?: boolean) => {
      const [fill, rest] = bar(p / 100, width)
      const rows = extra.filter((x): x is string => !!x)
      return {
        rows: 1 + (cols >= 28 ? 1 : 0) + rows.length + (fast === undefined ? 0 : 1),
        el: (
          <Box flexDirection="column">
            <Box justifyContent="space-between">
              <Text color="inactive">{label}</Text>
              <Text color={level(p)}>{pct(p)}</Text>
            </Box>
            {cols >= 28 && (
              <Text>
                <Text color={level(p)}>{fill}</Text>
                <Text color="subtle">{rest}</Text>
              </Text>
            )}
            {rows.map(r => (
              <Text color="inactive" wrap="truncate-end">{r}</Text>
            ))}
          </Box>
        ),
      }
    }

    const sections: { rows: number; el: RenderElement; optional?: true }[] = []
    if (s.context.window > 0) {
      const tokens = s.context.tokens === null ? '' : `${fmtTokens(s.context.tokens)} / ${fmtTokens(s.context.window)}`
      const compact = s.compactLeft === null ? '' : `Compact in ${fmtTokens(Math.max(0, s.compactLeft))}`
      const p = s.context.percent ?? ((s.context.tokens ?? 0) / s.context.window) * 100
      sections.push(gauge('KONTEXT', p, [[tokens, compact].filter(Boolean).join(' · ')]))
    }
    for (const kind of cols < 24 ? ['five_hour'] : ['five_hour', 'seven_day']) {
      const l = s.limits.find(l => l.kind === kind)
      const w = WINDOWS[kind]
      if (!l || !w) continue
      const g = gauge(w.label, l.percentUsed, [fmtReset(l.resetsAt, now)])
      const forecast = kind === 'five_hour' ? pace(l.percentUsed, l.resetsAt, w.ms, now) : undefined
      if (forecast !== undefined) {
        const fast = forecast > 100
        g.rows++
        g.el = (
          <Box flexDirection="column">
            {g.el}
            <Text wrap="truncate-end">
              <Text color={fast ? 'warning' : 'inactive'}>{fast ? '◆' : '◇'}</Text>
              <Text color="inactive">
                {fast ? ` zu schnell → 100% vor Reset` : ` im Plan → ~${Math.round(forecast)}% bei Reset`}
              </Text>
            </Text>
          </Box>
        )
      }
      sections.push(g)
    }

    if (cols >= 24) {
      const t = s.tokens
      const seen = t.input + t.cacheRead + t.cacheWrite
      const hours = (now - s.startedAt) / HOUR
      const details: [string, string | undefined][] = [
        ['Modell', s.model && [s.model.replace(/^claude-/, ''), s.effort].filter(Boolean).join(' · ')],
        // Im Abo bleibt cost 0, dann sagt die Zeile nichts.
        ['Kosten', !s.cost ? undefined : `$${s.cost.toFixed(2)}${hours > 0.05 ? ` · $${(s.cost / hours).toFixed(2)}/h` : ''}`],
        ['Tokens', seen || t.output ? `${fmtTokens(seen)} in · ${fmtTokens(t.output)} out` : undefined],
        ['Cache', seen ? `${Math.round((t.cacheRead / seen) * 100)}% Hit` : undefined],
        ['Dauer', s.startedAt ? `${fmtDuration(now - s.startedAt)} · ${s.turns} Turns` : undefined],
      ]
      const shown = details.filter((d): d is [string, string] => !!d[1])
      if (shown.length) sections.push({
        rows: shown.length,
        optional: true,
        el: (
          <Box flexDirection="column">
            {shown.map(([k, v]) => (
              <Text wrap="truncate-end">
                <Text color="inactive">{k.padEnd(12)}</Text>
                {v}
              </Text>
            ))}
          </Box>
        ),
      })

      if (s.active.length) {
        const icon = { run: ['●', 'claude'], ok: ['✓', 'success'], err: ['✗', 'error'] } as const
        sections.push({
          rows: 1 + s.active.length,
          optional: true,
          el: (
            <Box flexDirection="column">
                <Text color="inactive">AKTIV</Text>
              {s.active.map(a => (
                <Text wrap="truncate-end">
                  <Text color={icon[a.state][1]}>{icon[a.state][0]}</Text> {a.tool.padEnd(6)} {middle(a.target, Math.max(4, width - 9))}
                </Text>
              ))}
            </Box>
          ),
        })
      }

      if (s.todos.length) {
        const mark = { completed: ['✓', 'success'], in_progress: ['▸', 'claude'] } as Record<string, [string, string]>
        sections.push({
          rows: 1 + s.todos.length,
          optional: true,
          el: (
            <Box flexDirection="column">
              <Box justifyContent="space-between">
                <Text color="inactive">TODOS</Text>
                <Text color="inactive">
                  {s.todos.filter(t => t.status === 'completed').length}/{s.todos.length}
                </Text>
              </Box>
              {s.todos.map(t => (
                <Text wrap="truncate-end">
                  <Text color={(mark[t.status] ?? ['○', 'inactive'])[1]}>{(mark[t.status] ?? ['○'])[0]}</Text> {t.content}
                </Text>
              ))}
            </Box>
          ),
        })
      }
    }

    // Runde Karten, solange alles passt; sonst flach mit Leerzeilen. Optionale Sektionen fallen erst,
    // wenn auch flach nicht mehr passt (von unten: Todos, Aktiv, Details).
    const flatH = () => sections.reduce((n, x, i) => n + x.rows + (i ? 1 : 0), 0)
    const cardH = () => sections.reduce((n, x) => n + x.rows + 2, 0)
    while (sections.at(-1)?.optional && cardH() > scroll.bodyRows && flatH() > scroll.bodyRows) sections.pop()

    if (!sections.length) return <Text color="inactive">Noch keine Daten</Text>
    return cardH() <= scroll.bodyRows ? (
      <Box flexDirection="column">
        {sections.map(x => (
          <Box flexDirection="column" borderStyle="round" borderColor="subtle" paddingX={1}>
            {x.el}
          </Box>
        ))}
      </Box>
    ) : (
      <Box flexDirection="column" paddingX={1}>
        {sections.map((x, i) => (i ? <Box marginTop={1}>{x.el}</Box> : x.el))}
      </Box>
    )
  })
}
