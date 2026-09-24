---
name: release
description: Neues Open-Claude-Release erstellen (Version bumpen, testen, taggen, Draft anlegen, Release-Run überwachen). Nutzen, wenn der User "release", "neues Release", "Version raus" o. ä. sagt.
---

# Release erstellen

Ablauf von "was ist neu" bis "Draft mit allen Dateien". **Veröffentlichen macht der User selbst**, außer er sagt es explizit.

## 1. Was kommt rein?

```sh
git fetch -q && git status -sb | head -1        # sauber und synchron mit origin?
last=$(git describe --tags --abbrev=0)
git log --oneline $last..HEAD
```

- Nichts seit dem letzten Tag → kein Release, dem User sagen.
- Uncommittete Änderungen, die mit rein sollen → zuerst committen (siehe 3).
- Patch-Version hochzählen (1.0.6 → 1.0.7), außer der User will Minor/Major.

## 2. Tests (dieselben wie im Release-Workflow)

```sh
npm run check
node --test src/lib/*.test.ts
(cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test)
```

Irgendwas rot → stoppen, fixen, nicht taggen.

## 3. Version bumpen + committen

Die Version steht an **fünf** Stellen, alle ändern:

| Datei | Stelle |
|---|---|
| `package.json` | `"version"` |
| `package-lock.json` | `"version"` oben **und** unter `packages[""]` (Zeilen 3 und 9) |
| `src-tauri/Cargo.toml` | `version` in `[package]` |
| `src-tauri/Cargo.lock` | `version` beim Paket `name = "ocui"` (nicht cfg-if/dunce o. ä. mit gleicher Nummer!) |
| `src-tauri/tauri.conf.json` | `"version"` |

Danach `git diff --stat` → genau diese Dateien, je 1 Zeile (package-lock: 2).

Commit-Format (Changelog wird daraus generiert):

```
fix: Kurzbeschreibung, 1.0.7

- Stichpunkt, was Nutzer merken
- noch einer
- Version 1.0.7
```

- Präfix `feat:` oder `fix:` → nur deren `- `-Stichpunkte landen im Changelog. `chore:`/`ci:` tauchen nicht auf.
- `- Version X` wird aus dem Changelog gefiltert.
- **Kein** `Co-Authored-By` oder sonstiger Footer. Titel ≤ 50 Zeichen wenn möglich.
- Reiner Version-Bump ohne eigene Änderung: `chore: Version 1.0.7` mit den Stichpunkten der enthaltenen Fixes.

## 4. Taggen, pushen, Draft anlegen

```sh
git tag v1.0.7 && git push origin master v1.0.7
```

**Draft sofort danach selbst anlegen.** Das Actions-Token bekommt seit 1.0.6 beim *Anlegen* eines Releases 403 `Resource not accessible by integration` (Ursache unklar; Rechte, Settings, Rulesets waren alle ok). *Hochladen* in einen vorhandenen Draft klappt. Die Jobs brauchen ~5 min bis zum Upload, also reicht es direkt nach dem Push:

```sh
gh api repos/marlon-schmitz-289/open-claude/releases \
  -f tag_name=v1.0.7 -f name="Open Claude 1.0.7" -F draft=true \
  -f body="- Stichpunkt 1
- Stichpunkt 2"
```

Body = die `- `-Stichpunkte der feat/fix-Commits seit dem letzten Tag, ohne `- Version …`. tauri-action überschreibt den Body eines vorhandenen Drafts nicht.

## 5. Run überwachen

Push auf `master` startet **keine** CI mehr, nur der Tag startet `Release` (`.github/workflows/release.yml`, ~10 min, 4 Jobs: windows, macos-aarch64, macos-x86_64, linux). Im Hintergrund warten:

```sh
id=$(gh run list -w release.yml -L 1 --json databaseId -q '.[0].databaseId')
until s=$(gh run view $id --json status,conclusion -q '.status+" "+.conclusion') && [[ $s == completed* ]]; do sleep 30; done
echo "$s"; gh run view $id --log-failed | grep -E "##\[error\]|FAILED|panicked" | cut -f1,3
gh release view v1.0.7 --json isDraft,assets -q '"draft=\(.isDraft) assets=\(.assets|length)"'
```

Erwartung: `success`, **15 Assets** (2 DMGs, exe+sig, msi+sig, deb+sig, AppImage+sig, 2× app.tar.gz+sig, latest.json).

Fehler:
- 403 beim Create → Draft fehlt oder Tag-Name falsch; Draft anlegen, `gh run rerun $id --failed`.
- Tests rot → fixen, neuen Patch-Release (Tags nicht verschieben, sobald gepusht).
- Einzelner Job transient rot → `gh run rerun $id --failed`.

## 6. Übergabe

Dem User melden: Version, was drin ist, Run grün, 15 Assets im Draft. Er veröffentlicht den Draft (erst dann sehen Auto-Updates die Version).
