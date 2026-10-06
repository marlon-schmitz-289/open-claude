import { test } from "node:test";
import assert from "node:assert/strict";
import { evenCell } from "./termcell.ts";

// Zellgroesse wie WebglRenderer._updateDimensions (addon-webgl 0.19).
const cell = (charW: number, charH: number, dpr: number, o: { letterSpacing: number; lineHeight: number }) => {
  const ch = Math.ceil(charH * dpr);
  return { w: Math.floor(charW * dpr) + Math.round(o.letterSpacing), h: Math.floor(ch * o.lineHeight), ch };
};

// 13 px: Cascadia Code, Consolas, SF Mono (Breite, Hoehe in CSS-Pixeln, gerundet).
const fonts = [
  [7.617, 15.107],
  [7.147, 15.22],
  [7.83, 15.5],
];

test("evenCell: Zelle in Geraetepixeln gerade, DPR 1/1.25/1.5/2 mal Zoom 50-200 %", () => {
  let odd = 0;
  for (const [cw, chh] of fonts) {
    for (const scale of [1, 1.25, 1.5, 2]) {
      for (let zoom = 50; zoom <= 200; zoom += 10) {
        const dpr = (scale * zoom) / 100;
        const before = cell(cw, chh, dpr, { letterSpacing: 0.3, lineHeight: 1.2 });
        if (before.w % 2 || before.h % 2) odd++;
        const o = evenCell(cw, chh, dpr);
        const c = cell(cw, chh, dpr, o);
        const at = `dpr ${dpr} font ${cw}`;
        assert.equal(c.w % 2, 0, `Breite ${c.w} bei ${at}`);
        assert.equal(c.h % 2, 0, `Hoehe ${c.h} bei ${at}`);
        // Hoechstens ein Geraetepixel groesser, nie kleiner als das Zeichen.
        assert.ok(c.w - before.w === 0 || c.w - before.w === 1, at);
        assert.ok(c.h - before.h === 0 || c.h - before.h === 1, at);
        assert.ok(o.lineHeight >= 1 && c.h >= c.ch, at);
      }
    }
  }
  // Ohne Korrektur sind viele Stufen ungerade, sonst testet das hier nichts.
  assert.ok(odd > 50, `${odd}`);
});

test("evenCell: gerade Zelle bleibt unveraendert", () => {
  assert.deepEqual(evenCell(8, 15, 1), { letterSpacing: 0, lineHeight: 1.2 });
  assert.deepEqual(evenCell(7.5, 15, 1), { letterSpacing: 1, lineHeight: 1.2 });
  assert.equal(Math.floor(16 * evenCell(8, 16, 1).lineHeight), 20);
});
