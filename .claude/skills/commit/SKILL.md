---
name: commit
description: Änderungen committen und pushen – ein Commit pro Feature/Fix, kurze Titelzeile, Stichpunkte, keine Co-Author-Footer. Nutzen, wenn der User "commit", "committen", "push", "einchecken" o. ä. sagt.
---

# Committen und pushen

## 1. Überblick

```sh
git status -sb && git diff --stat && git diff --cached --stat
```

Diffs lesen und Änderungen nach **Feature/Fix** gruppieren. Jedes Feature, jeder Fix = **eigener Commit**. Nicht zusammenpacken, was unabhängig ist; nicht zerreißen, was zusammengehört (Feature + seine Tests + seine Doku = ein Commit).

## 2. Pro Gruppe: stagen und committen

- Ganze Dateien: `git add <dateien>`
- Zwei Features in **einer** Datei (kein interaktives `git add -p` möglich):
  ```sh
  git diff <datei> > "$TMP/p.diff"   # Hunks des anderen Features rauslöschen
  git apply --cached "$TMP/p.diff"
  ```
  Danach `git diff --cached` prüfen: nur dieses Feature drin?

Commit-Message:

```
fix: Login-Timeout konfigurierbar

- Timeout aus Config statt hardcoded 30s
- Fehlermeldung bei Timeout verständlicher
```

- **Titel** ≤ 50 Zeichen, mit Präfix: `feat:` neues Feature, `fix:` Bugfix, `chore:` Wartung/Versionen, `ci:` Workflows, `docs:` Doku, `refactor:` ohne Verhaltensänderung.
- **Stichpunkte** kurz und knapp: was Nutzer merken bzw. was sich ändert, nicht wie der Code aussieht. Bei `feat:`/`fix:` landen sie 1:1 im Release-Changelog → so formulieren, dass sie dort Sinn ergeben.
- Kein Body-Text außer Stichpunkten. Winzige Änderung → Titel reicht.
- **Niemals** `Co-Authored-By`, "Generated with …", Signaturen oder sonstige Footer.
- Sprache: Deutsch, wie die bisherige History.
- Mehrzeilige Message per `git commit -m "Titel" -m "- a
- b"` bzw. Heredoc, nicht per Editor.

Version **nicht** anfassen – das macht der `release`-Skill.

## 3. Checks

Vor dem ersten Commit einmal `npm run check`; bei Rust-Änderungen zusätzlich `(cd src-tauri && cargo clippy --all-targets -- -D warnings)`. Rot → erst fixen oder dem User melden, nicht rot committen.

## 4. Pushen

Direkt nach dem letzten Commit, vorher die neuen Commits für die Meldung merken:

```sh
git log --oneline @{u}..HEAD
git push origin HEAD
```

Push auf `master` startet keine Action (CI nur bei PRs, Release nur bei Tags). Push abgelehnt (remote weiter) → `git pull --rebase`, dann erneut pushen; bei Konflikten stoppen und dem User melden. **Nie** `--force`.

## 5. Meldung

Dem User die gepushten Commits (Hash + Titel) auflisten, fertig.
