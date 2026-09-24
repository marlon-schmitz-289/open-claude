# Open Claude

Desktop-Launcher für Claude Code. Findet alle Git-Repos unter einem Dev-Ordner und startet
`claude` im gewählten Projekt in einem eingebauten Terminal (mit der eigenen Shell `ocui-sh`).
Dazu kommt ein eingebauter Git-Client: Änderungen stagen und committen, Verlauf mit Graph,
Branches, Stashes, Tags, Merge-Editor und Klonen von GitHub und GitLab.

Tauri v2 (Rust) + SvelteKit + shadcn-svelte.

## Installation

### Windows

Setup aus den [Releases](../../releases) laden und ausführen. Die Anwendung ist nicht signiert,
Windows fragt daher einmalig nach: *Weitere Informationen → Trotzdem ausführen*.

Voraussetzungen auf dem Rechner:

- `claude` auf dem PATH — sonst öffnet sich das Terminal ohne startende CLI
- Git for Windows — für die Git-Ansicht, die Zeit des letzten Commits und für Befehle mit Pipes,
  Umleitungen oder Variablen im Terminal (`ocui-sh` reicht sie an die Git-Bash weiter, sonst an `cmd`)
- Windows 10 1809 oder neuer (ConPTY)

### macOS

`.dmg` (Apple Silicon oder Intel) aus den [Releases](../../releases) laden. Die Anwendung ist
nicht signiert, macOS blockiert den ersten Start daher: im Finder mit Rechtsklick auf die App
→ *Öffnen* wählen, oder per Terminal die Quarantäne-Markierung entfernen:

```sh
xattr -dr com.apple.quarantine "/Applications/Open Claude.app"
```

Voraussetzung: `claude` auf dem PATH, sonst öffnet sich das Terminal ohne startende CLI.

### Linux

AppImage oder `.deb` aus den [Releases](../../releases) laden. Das AppImage braucht
Ausführrechte (`chmod +x`), das `.deb` lässt sich mit `apt install ./open-claude_*.deb`
einrichten.

Voraussetzungen auf dem Rechner:

- `claude` auf dem PATH — sonst öffnet sich das Terminal ohne startende CLI
- WebKitGTK (`libwebkit2gtk-4.1-0`) und `libayatana-appindicator3-1` für das Tray-Icon,
  auf gängigen Distributionen meist schon vorhanden

## Bedienung

Tippen filtert unscharf: `adbe` findet `adesk-backend`, `uber` findet `Überblick`. Auch Sprachen sind
suchbar (`rust`).

| Taste | Aktion |
| --- | --- |
| `⏎` / Klick | Git-Ansicht öffnen |
| `Strg` + `⏎` / `Strg` + Klick | Claude im Projekt starten |
| `↑` `↓` | Projekt wählen |
| `Strg` + `1`…`9` | Claude im Treffer starten |
| `Strg` + `G` | Git-Ansicht öffnen |
| `Strg` + `N` | Repo klonen |
| `Strg` + `K` | Suche fokussieren |
| `Strg` + `P` | Projekt anpinnen |
| `Strg` + `E` | Ordner im Explorer öffnen |
| `Strg` + `R` | Neu einlesen |
| `Strg` + `O` | Dev-Ordner wechseln |
| `F1` | Kürzel-Übersicht |
| `Esc` | Suche leeren, sonst schließen (je nach Einstellung ins Tray) |

Im Terminal gehen alle Tasten an die Shell, außer:

| Taste | Aktion |
| --- | --- |
| `Shift` + `Esc` oder `Strg` + `Shift` + `W` | Zurück zur Liste, Sitzung läuft weiter |
| `Alt` + `G` | Git-Ansicht des Projekts |
| `Alt` + `E` | Ordner im Explorer öffnen |
| `Strg` + `Shift` + `C` | Auswahl kopieren |
| `Strg` + `V` | Einfügen |

Angepinnte Projekte stehen immer oben. Die Sprache wird an den üblichen Projektdateien erkannt
(`Cargo.toml`, `*.csproj`, `tsconfig.json`, …) und als Badge angezeigt, daneben Branch und Alter
des letzten Commits.

Beim ersten Start fragt die App nach dem Dev-Ordner, vorbelegt mit `%DEV%`, sonst `%USERPROFILE%\Dev`;
später wechselt `Strg` + `O` ihn. Einstellungen,
Pins und der Repo-Cache liegen in `%APPDATA%\com.marlonschmitz.openclaude\settings.json`.

Pro Projekt läuft höchstens eine Sitzung. Erneutes Starten springt zur offenen Sitzung,
laufende Sitzungen sind in der Liste markiert. Beenden (Titelleiste oder das X am Badge *läuft*)
fragt vorher nach. Das Terminal startet `ocui-sh`, die neben der App liegt: erst `claude`, danach
eine einfache Shell. Fehlt sie, nimmt die App Git-Bash, `pwsh` oder `cmd`.

Das Zahnrad rechts in der Suchleiste hat unter Windows und Linux zwei Schalter:

- *Schließen legt ins Tray* (Standard: an): Schließen versteckt das Fenster nur, es kommt per
  Klick aufs Tray-Icon zurück; beendet wird die App über *Beenden* im Kontextmenü des Tray-Icons.
  Ist der Schalter aus, beendet Schließen die App samt allen laufenden Sitzungen.
- *Autostart* startet die App bei der Anmeldung des Benutzers.

Unter macOS gibt es beide nicht: Schließen versteckt das Fenster, ein Klick aufs Dock- oder Menüleisten-Icon holt es
zurück, Cmd+Q beendet die App. Autostart läuft dort über die Anmeldeobjekte der Systemeinstellungen.

Daneben liegen *Neu einlesen*, *Klonen* und *Konten* als Icons.

## Git-Ansicht

`Strg` + `G` in der Liste oder *Git* im Terminal-Kopf öffnet die Git-Ansicht des Projekts.
Links stehen Branches (mit ↑↓ zum Upstream), Remotes, Tags, Stashes und offene Pull Requests,
rechts die Tabs *Änderungen* und *Verlauf*. Aktionen gibt es per Rechtsklick, Löschen,
Reset, Verwerfen und Force-Push fragen vorher nach. Nicht gemergte Branches lassen sich nach
einer zweiten Rückfrage trotzdem löschen.

- **Nicht gemergt:** Oben in der Sidebar stehen alle Branches mit Commits, die noch nicht im
  Production-Branch sind, mit Anzahl und Alter. Gemergte Branches sind ausgegraut. Der
  Production-Branch wird erkannt (`origin/HEAD`, sonst `main`/`master`) oder übers Zahnrad bzw.
  Rechtsklick auf einen Branch fest eingestellt; das steht pro Repo in `git config ocui.base`.

- **Änderungen:** Doppelklick oder Leertaste stagt bzw. unstagt eine Datei. Im Diff lassen sich
  einzelne Hunks oder per Klick/Umschalt-Klick einzelne Zeilen stagen, unstagen und verwerfen.
  Commit mit `Strg` + `⏎`, *Amend* lädt die letzte Nachricht.
- **Verlauf:** Commit-Graph über alle Branches, Suche nach Text, Autor oder SHA (lädt dafür ältere
  Commits nach, bis 5000 automatisch, danach per Klick). Rechtsklick auf
  einen Commit: Branch oder Tag anlegen, auschecken, Cherry-Pick, Revert, Reset.
- **Konflikte:** Bei Merge, Rebase, Cherry-Pick oder Revert zeigt ein Banner die offenen
  Konflikte mit *Fortsetzen* und *Abbrechen*. Klick auf eine Konfliktdatei öffnet den
  Merge-Editor: pro Block Ours, Theirs, beide oder Basis wählen, `F7` springt zum nächsten
  Konflikt, `Strg` + `S` speichert und markiert die Datei als gelöst.
- *Claude*, *Explorer* und Aktualisieren stehen oben in der Titelleiste.

| Taste | Aktion |
| --- | --- |
| `Strg` + `1` / `2` | Änderungen / Verlauf |
| `Strg` + `Shift` + `F` | Fetch |
| `Strg` + `Shift` + `L` | Pull (Merge; Rebase über das Menü daneben) |
| `Strg` + `Shift` + `P` | Push, ohne Upstream mit `-u origin` |
| `F5` | Aktualisieren, passiert auch bei Fensterfokus |
| `Esc` oder `Strg` + `Shift` + `W` | Zurück zur Liste |

Fetch, Pull und Push melden sich über den Git Credential Manager an, der bei Git for Windows
dabei ist.

## Konten und Klonen

*Konten* (Icon in der Suchleiste) verwaltet GitHub- und GitLab-Konten, auch selbst gehostete
(Host-Feld, z. B. `gitlab.firma.de`). Anmelden geht per Personal Access Token oder mit
*Von gh/glab übernehmen*, wenn die jeweilige CLI angemeldet ist. Nötige Token-Rechte:

| Anbieter | Scopes |
| --- | --- |
| GitHub | `repo`, `read:org` |
| GitLab | `read_api`, `read_repository` |

Der Token liegt nur in der Windows-Anmeldeinformationsverwaltung (Dienst `ocui`), die
Kontoliste ohne Token in `settings.json`.

*Klonen* (`Strg` + `N`) listet die eigenen Repos des gewählten Kontos, zuletzt aktive zuerst.
Tippen filtert lokal, `⏎` bzw. *Überall suchen* sucht auf dem Server. Alternativ eine beliebige
Git-URL einfügen. Zielordner ist `<Dev-Ordner>/<Name>`, wählbar sind HTTPS oder SSH. Beim
Klonen über HTTPS wird der Token nur für diesen einen Aufruf mitgegeben und nirgends
gespeichert. Danach liest die App neu ein und markiert das Projekt.

## Entwicklung

```sh
npm install
npm run tauri dev

npm run check                          # Svelte + TypeScript
node --test src/lib/*.test.ts          # Suche, Diff, Graph, Konflikte
cd src-tauri && cargo test             # Scan, Git-Parser, Forge
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

## Releases bauen

Windows-, macOS- (Apple Silicon und Intel) und Linux-Builds laufen kostenlos über
GitHub Actions (`.github/workflows/release.yml`). Ein Tag `vX.Y.Z` pushen löst den Build
aus und legt einen Entwurf unter [Releases](../../releases) mit allen Installern an; der
Entwurf muss danach manuell freigegeben werden. Über *Actions → Release → Run workflow*
lässt sich derselbe Workflow auch ohne Release nur zum Testen der Builds anstoßen, die
Bundles liegen dann als Artefakte am Workflow-Lauf.
