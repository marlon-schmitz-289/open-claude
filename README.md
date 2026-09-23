# Open Claude

Desktop-Launcher für Claude Code. Findet alle Git-Repos unter einem Dev-Ordner und startet
`claude` im gewählten Projekt in einem neuen Terminal-Fenster.

Tauri v2 (Rust) + SvelteKit + shadcn-svelte.

## Installation

Setup aus den [Releases](../../releases) laden und ausführen. Die Anwendung ist nicht signiert,
Windows fragt daher einmalig nach: *Weitere Informationen → Trotzdem ausführen*.

Voraussetzungen auf dem Rechner:

- `claude` auf dem PATH — sonst öffnet sich das Terminal ohne startende CLI
- `git` auf dem PATH — sonst fehlt nur die Zeit des letzten Commits

## Bedienung

Tippen filtert unscharf: `adbe` findet `adesk-backend`. Auch Sprachen sind suchbar (`rust`).

| Taste | Aktion |
| --- | --- |
| `⏎` | Claude im Projekt starten |
| `↑` `↓` | Projekt wählen |
| `Strg` + `1`…`9` | Treffer direkt starten |
| `Strg` + `K` | Suche fokussieren |
| `Strg` + `P` | Projekt anpinnen |
| `Strg` + `E` | Ordner im Explorer öffnen |
| `Strg` + `R` | Neu einlesen |
| `Strg` + `O` | Dev-Ordner wechseln |
| `F1` | Kürzel-Übersicht |
| `Esc` | Suche leeren, sonst in den Tray |

Angepinnte Projekte stehen immer oben. Die Sprache wird an den üblichen Projektdateien erkannt
(`Cargo.toml`, `*.csproj`, `tsconfig.json`, …) und als Badge angezeigt, daneben Branch und Alter
des letzten Commits.

Ohne gesetzten Pfad sucht die App in `%DEV%`, sonst in `%USERPROFILE%\Dev`. Einstellungen,
Pins und der Repo-Cache liegen in `%APPDATA%\com.marlonschmitz.openclaude\settings.json`.

Schließen legt die App nur ins Tray ab, das Fenster kommt per Klick aufs Tray-Icon zurück.
Wirklich beendet wird sie über *Beenden* im Kontextmenü des Tray-Icons.

Der Autostart-Schalter in der Fußzeile trägt die App in den Autostart des angemeldeten
Benutzers ein.

## Entwicklung

```sh
npm install
npm run tauri dev

npm run check                          # Svelte + TypeScript
node --test src/lib/fuzzy.test.ts      # Suche
cd src-tauri && cargo test             # Scan und Spracherkennung
```

Bauen:

```sh
npm run tauri build -- --no-bundle       # nur die .exe
npm run tauri build -- --bundles nsis    # Setup
npm run tauri build -- --bundles msi     # MSI für die Softwareverteilung
```

Hinter einem Proxy mit eigenem Root-Zertifikat brauchen die Paketmanager einen Hinweis:
`.cargo/config.toml` setzt dafür `check-revoke = false`, und für die shadcn-Registry hilft
`NODE_OPTIONS=--use-system-ca`.
