/**
 * xterm-Optionen, mit denen die WebGL-Zelle in Geraetepixeln gerade Breite und Hoehe hat.
 * Rechnet wie WebglRenderer._updateDimensions (addon-webgl): Breite floor(charW*dpr) + round(letterSpacing),
 * Hoehe floor(ceil(charH*dpr) * lineHeight). Ungerade Zellen legen die Halb- und Quadrantenkanten der
 * selbstgezeichneten Blockzeichen auf halbe Pixel: Haarlinien im Logo.
 * charW/charH in CSS-Pixeln (CharSizeService). Gerade Zellen behalten lineHeight unveraendert.
 */
export function evenCell(charW: number, charH: number, dpr: number, lineHeight = 1.2) {
  const ch = Math.ceil(charH * dpr);
  const h = Math.floor(ch * lineHeight);
  return {
    letterSpacing: Math.floor(charW * dpr) % 2,
    // +0.5 Pixel Luft, damit floor() trotz Gleitkomma sicher auf h + 1 landet.
    lineHeight: h % 2 ? (h + 1.5) / ch : lineHeight,
  };
}
