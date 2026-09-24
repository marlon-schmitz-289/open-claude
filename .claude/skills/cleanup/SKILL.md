---
name: cleanup
description: Build-Artefakte löschen, um Plattenplatz freizugeben (v. a. src-tauri/target). Nutzen nach einem Commit/Push, nach Tests oder Builds, oder wenn der User "cleanup", "aufräumen", "Speicher freigeben" sagt.
---

# Aufräumen

Speicher ist auf dem System knapp. Rust-Builds legen in `src-tauri/target` schnell 3–6 GB ab, ein Release-Build nochmal ~2 GB.

## Wann

- Nach dem `commit`-Skill (nach dem Push), wenn in der Session Rust gebaut/getestet wurde.
- Nach lokalen Builds/Messungen (`cargo build --release`, `npm run tauri build`).
- Nicht, solange der User gerade `npm run tauri dev` laufen hat oder direkt weitere Rust-Änderungen anstehen – dann erst am Ende.

## Ablauf

```sh
df -h / | tail -1
du -sh src-tauri/target build .svelte-kit 2>/dev/null
(cd src-tauri && cargo clean)
rm -rf build
df -h / | tail -1
```

- `src-tauri/target`: per `cargo clean`, nicht per `rm`.
- `build/`: Frontend-Output, entsteht bei `npm run build` neu.
- **Nicht** löschen: `node_modules` (klein, `npm ci` dauert), `.svelte-kit` (winzig, braucht `npm run check`), `src-tauri/gen` (Tauri-Schemas).
- **Nie** `git clean` oder ungetrackte Dateien löschen – da kann unfertige Arbeit liegen.
- Temporäre Dateien dieser Session nur im Scratchpad-Verzeichnis, nicht im Repo.

## Meldung

Eine Zeile: wie viel frei wurde und wie viel jetzt frei ist (z. B. „4,3 GB freigegeben, 5,9 GB frei“). Hinweis, dass der nächste Rust-Build/-Test dadurch einmal länger dauert.
