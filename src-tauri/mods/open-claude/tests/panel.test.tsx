import { expect, mock, test } from 'claude-code/testing'

import { bar, level, pace } from '../hooks/register'

const HOUR = 3_600_000

test('bar', () => {
  // 0.62 * 33 = 20.46 Zellen: 20 volle plus Halbzelle
  expect(bar(0.62, 33)).toEqual(['━'.repeat(20) + '╸', '─'.repeat(12)])
  expect(bar(0, 4)).toEqual(['', '────'])
  expect(bar(1.5, 4)).toEqual(['━━━━', ''])
})

test('pace', () => {
  const now = Date.parse('2026-10-08T12:00:00Z')
  // halbes 5h-Fenster vorbei, 38 % verbraucht
  expect(pace(38, new Date(now + 2.5 * HOUR).toISOString(), 5 * HOUR, now)).toBe(76)
  // Fenster beginnt erst: keine Prognose
  expect(pace(10, new Date(now + 5 * HOUR).toISOString(), 5 * HOUR, now)).toBe(undefined)
  expect(pace(10, undefined, 5 * HOUR, now)).toBe(undefined)
})

test('level', () => {
  expect([level(49), level(50), level(80)]).toEqual(['claude', 'warning', 'error'])
})

const PROPS = {
  title: 'Session',
  isFocused: false,
  bodyColumns: 36,
  placement: 'dock' as const,
  scroll: { offset: 0, bodyRows: 40 },
  view: {},
}

test('pane', async ($, on) => {
  on('session.measure', (_$, e) => ({ changed: e.changed }))
  on('clock.now', () => ({ value: Date.parse('2026-10-08T12:00:00Z') }))
  await $.session.measure({
    context: { window: 200000, tokens: 124000, percent: 62 } as never,
    rateLimits: [{ kind: 'five_hour', percentUsed: 38 }, { kind: 'seven_day', percentUsed: 21 }],
    changed: ['context', 'rateLimits'],
  })
  const ui = await $.ui.mount({ plugin: 'open-claude', surface: 'terminal', component: 'Pane', props: PROPS, requestId: 'session' })
  expect(await ui.find({ text: /62%/ })).toBeDefined()
  expect(await ui.find({ text: /WOCHE/ })).toBeDefined()
  expect(await ui.find({ text: /─{5}/ })).toBeDefined()
  expect(JSON.stringify(await ui.drawn())).toContain('"borderStyle":"round"')

  await ui.redraw({ ...PROPS, scroll: { offset: 0, bodyRows: 6 } })
  expect(JSON.stringify(await ui.drawn())).not.toContain('borderStyle')
  expect(await ui.find({ text: /KONTEXT/ })).toBeDefined()

  await ui.redraw({ ...PROPS, bodyColumns: 22 })
  expect(await ui.find({ text: /─{5}/ })).toBe(undefined)
  expect(await ui.find({ text: /WOCHE/ })).toBe(undefined)

  await ui.redraw({ ...PROPS, placement: 'inline' })
  const tree = await ui.drawn()
  expect(tree).toMatchObject({ type: 'Text' })
  expect((await ui.find({ type: 'Text' }))?.text).toBe('Ctx 62% · 5h 38% · 7d 21%')
})

test('datei fuers app-panel', async ($, on) => {
  mock.env(on, { OPEN_CLAUDE_PANEL: '/tmp/x/p.json' })
  const w: { path: string; text: string }[] = []
  on('fs.write', (_$, e) => (w.push(e), { value: undefined }))
  on('session.measure', (_$, e) => ({ changed: e.changed }))
  await $.session.measure({
    context: { window: 200000, tokens: 50000, percent: 25 } as never,
    rateLimits: [{ kind: 'five_hour', percentUsed: 10 }],
    changed: ['context', 'rateLimits'],
  })
  expect(w.at(-1)?.path).toBe('/tmp/x/p.json')
  expect(JSON.parse(w.at(-1)!.text)).toMatchObject({ context: { percent: 25 }, limits: [{ percentUsed: 10 }] })
})
