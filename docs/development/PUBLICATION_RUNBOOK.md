# Publication Runbook: Making Runlith a Public Repository

> **Owner-executed. Not run by agents.** This runbook documents the steps for
> the repository owner to take the private `fallowlone/Runlith` repository
> public. A coding agent may read this file and may prepare evidence for the
> preconditions below, but must not run the export/push commands in this
> file, create a GitHub repository, or push to one.

**The existing private repository must never be switched to public
directly** (for example via the GitHub repository visibility toggle in
Settings). Its git history contains, permanently, the four revoked NVIDIA
API keys committed at `56b8a68` in plaintext, and every commit before that
point carries no `LICENSE` at all while every commit from `56b8a68` onward
up to the ADR-0038 branch carries the Apache Shield 1.0.0 `LICENSE` (see
Section 5 for the full history). Publication happens only by exporting a
fresh, history-less tree (Section 4) into a brand-new repository; the
private `fallowlone/Runlith` repository itself stays private, permanently.

## 1. Purpose

Publishing Runlith turns a private, open source repository into a
public one under [Apache License, Version 2.0](../../LICENSE) (see
[ADR-0038](../architecture/adr/ADR-0038-product-licensing.md)). This is a
one-way, high-consequence action: it starts each shipped version's four-year
Apache-2.0 License Date clock (ADR-0038 D4) and exposes the tree to the public
internet. Do it deliberately, not as a side effect of another task.

## 2. Preconditions checklist

Do not proceed to Section 3 until every item below is checked.

- [x] **ADR-0038 is merged to the repository default branch (`main`).**
      ADR-0038 (Apache License, Version 2.0) merged to `main` via commit
      `c040b6d` (PR #10). This task's own publication-readiness hygiene
      (TASK-0154) subsequently merged to `main` via commit `2779b70`
      (PR #11). `main`'s `LICENSE` file now carries Apache-2.0 1.1, not the earlier
      Apache Shield text.
- [x] **The NVIDIA API keys are revoked.** Four `nvapi-`-prefixed keys were
      committed in `Support/openvidia/proxy_config.json` at commit
      `56b8a68`. Removing the file from the tree (TASK-0154) does **not**
      remove it from history, and this runbook does not rewrite history (see
      Section 9). The owner confirmed revocation of all four keys with the
      NVIDIA provider on 2026-09-26. This was an owner action; an agent was
      never asked to confirm key validity, since doing so would require
      reading the secret.
- [ ] **`gitleaks dir` and `gitleaks git` on the export, with this
      repository's `.gitleaks.toml`, both return 0 findings.** Run the
      commands in Section 4.3 against the freshly exported tree (not the
      live history of this private repository) and confirm a clean result
      before pushing.
- [ ] **No `/Users/...` (or other contributor-machine) absolute paths remain
      in tracked files.** `git grep -n -I -E '/Users/[A-Za-z0-9_]'` returns
      nothing.
- [ ] **`CONTRIBUTING.md` and `SECURITY.md` are present** at the repository
      root and describe the current (no external-PR, Apache-2.0, private-vuln-report)
      posture.
- [ ] **Qualified legal review of the License Grant wording,
      copyright ownership and the copyright status of any AI-assisted
      authorship in this repository has been obtained — or the owner has
      explicitly accepted the risk of publishing without it and updated
      `LICENSE` accordingly.** [`LICENSE`](../../LICENSE)
      ("Contributions, trademarks and release readiness") already calls for
      exactly this review before public distribution; this precondition
      operationalizes that existing requirement rather than adding a new
      one. This is an owner decision that an agent cannot complete or waive
      on the owner's behalf.

## 3. Why a new repository with no history

Publishing the *existing* git history, even after removing the stray files
from the working tree, would publish every earlier commit as-is: every
commit from `56b8a68` up to the ADR-0038 branch carries the **Apache
Shield 1.0.0** `LICENSE` in effect at that commit, not Apache-2.0 1.1, and that
grant cannot be revoked retroactively once published (ADR-0038 D8: "this ADR
does not attempt to retroactively alter any grant"); and commit `56b8a68`
contains the four now-revoked NVIDIA API keys in plaintext, permanently, in
that commit's blob, regardless of what later commits do. See Section 5 for
the complete license-history timeline.

The chosen alternative is to **export the current `main` tree only, with no
history**, into a brand-new repository whose first commit is the export
itself. This publishes exactly one license (Apache-2.0 1.1, as merged) and no
historical secret material, at the cost of forfeiting the old history's
authorship record on the public remote; the private repository keeps that
history.

## 4. Step-by-step (owner-executed)

Run these from a clean checkout of `main` on the machine you intend to
publish from, **after** every Section 2 box is checked. Explanations are
given as prose above each command, never as inline comments inside the
command blocks (some shells choke on `#` inside here-docs, and it keeps the
commands copy-pasteable).

### 4.1 Export the current tree, no history

Create a fresh, empty directory with `mktemp -d` rather than a fixed path
such as `/tmp/runlith-public-export` — a fixed path may already exist from a
previous run and may already contain a `.git` directory or leftover files,
which would silently contaminate the export. Then archive the exact tree
that will become the public repository's first commit into that fresh
directory:

```sh
RUNLITH_EXPORT_DIR="$(mktemp -d)"
git archive --format=tar HEAD | tar -x -C "$RUNLITH_EXPORT_DIR"
cd "$RUNLITH_EXPORT_DIR"
git init
```

### 4.2 Set the commit identity and commit

Use a GitHub `noreply` identity so the export commit does not carry a
personal email address into a public, permanent commit. Replace `<login>`
with the owner's actual GitHub username before running:

```sh
git config user.name "<login>"
git config user.email "<login>@users.noreply.github.com"
git add -A
git commit -m "Initial public release"
```

Immediately after committing, record in your own notes (not necessarily in
this repository) the version identifier (`product/version.toml`) and
today's date as that version's **first public distribution date** — see
Section 6.

### 4.3 Verify the export before pushing

Run every check below against the freshly committed export from 4.2, while
still inside `$RUNLITH_EXPORT_DIR`, before doing anything in Section 4.4. If
any check fails, stop, do not push, fix the underlying cause in the source
tree that `main` was exported from (not just this export directory), and
restart from Section 4.1.

Confirm the export really has exactly one commit, with no leftover history
from a previous run of this runbook:

```sh
git rev-list --count HEAD
```

Expected: `1`.

Confirm the exported tree carries none of the tracked secrets this
repository's `.gitleaks.toml` knows to look for, scanning both the export's
own (single-commit) history and its working tree:

```sh
gitleaks git . --config .gitleaks.toml --redact
```

Expected: no leaks reported.

```sh
gitleaks dir . --config .gitleaks.toml --redact
```

Expected: no leaks reported.

Confirm no contributor-machine absolute path remains:

```sh
git grep -n -I -E '/Users/[A-Za-z0-9_]'
```

Expected: no output.

Confirm every commit in the export carries only the `noreply` identity set
in 4.2, never a personal email address:

```sh
git log --format='%an <%ae>'
```

Expected: a single line, repeated once per commit, showing only the
`<login>` and `<login>@users.noreply.github.com` identity configured above.

### 4.4 Create the new public GitHub repository and push

Create the new repository through the GitHub UI or `gh repo create`
(public, no README/license/gitignore auto-generated, since the export
already has them), then, still inside `$RUNLITH_EXPORT_DIR`:

```sh
git remote add origin <new-repository-git-url>
git push -u origin HEAD:main
```

## 5. License history and post-push confirmation

License history, for reference when reasoning about what publishing the
full history would expose (Section 3): commits before `56b8a68` carry no
`LICENSE` file at all; `56b8a68` onward carries **Apache Shield 1.0.0**;
on the ADR-0038 topic branch, `69b2221` briefly carried a Apache
open-source draft, superseded at `b534030` and onward on that branch by
**Apache License, Version 2.0**; `main` itself switched from Shield to Apache-2.0
1.1 only at the merge commit `c040b6d`. The conclusion from Section 3 is
unaffected by these details: do not publish the existing history.

After the push in Section 4.4 completes, clone the new public repository
into another fresh `mktemp -d` directory and re-run every check from
Section 4.3 (rev-list count, `gitleaks git`, `gitleaks dir`, the `/Users/`
grep, and the commit-identity log) against that clone. This confirms the
push transported exactly what was verified pre-push, with nothing added or
changed in transit.

## 6. Version / License Date bookkeeping

Apache-2.0 1.1 applies separately to each version, and each version's four-year
License Date runs from **that version's own first public distribution**
(ADR-0038 D4; "undefined 'version' for License Date bookkeeping" is an open
question in ADR-0038's Migration impact). Until this runbook is run, no
version has been publicly distributed and no clock has started. Each time
this runbook (or any later re-export) is run for a new version, record, in a
durable location the owner controls:

- the version identifier published (`product/version.toml` at export time);
- the date of first public distribution (today's date, if this is the
  first push of that version);
- the resulting License Date (four years later).

This record is not currently automated and is not itself part of this repo's
tracked state; treat it as owner-maintained bookkeeping until a follow-up
task gives it a durable home.

## 7. GitHub settings to enable after the push

- **Private vulnerability reporting** (Settings → Security → "Private
  vulnerability reporting"): enable it so `SECURITY.md`'s reporting channel
  actually exists.
- **Branch protection on `main`** (Settings → Branches): require the CI
  checks that apply, and disallow force-push/direct pushes once the
  workflow to protect is decided.
- **Actions**: confirm Actions are enabled for the new repository (Settings
  → Actions → General).
- **Discussions** (Settings → General → Features → "Discussions"):
  `CONTRIBUTING.md` ("Issues and discussions") already invites discussion
  via GitHub Discussions alongside Issues, so enable it on the new public
  repository to match; if the owner decides against hosting Discussions,
  update `CONTRIBUTING.md` to stop inviting them instead of leaving the
  feature disabled but promised.

GitHub Actions minutes on **standard GitHub-hosted runners are free (not
metered against a paid quota) for public repositories**; source:
`github/docs`, `content/billing/concepts/product-billing/github-actions.md`,
read 2026-09-26. This is relevant because, as of this writing, the private
repository's own Actions minutes quota is exhausted; a public repository
does not inherit that constraint for standard-hosted runners.

## 8. Trademark self-check (owner action)

Before or shortly after publication, self-check the "Runlith" name against
public trademark search tools — a self-check, not a legal clearance
opinion: EUIPO eSearch plus (EU), WIPO Global Brand Database
(international), USPTO TESS / Trademark Search (US). No trademark policy
or registration exists in this repository as of this writing (see
`LICENSE`, "Contributions, trademarks and release readiness").

## 9. Non-goals of this runbook

This runbook does not rewrite the private repository's git history, does
not revoke the NVIDIA keys (owner action, outside git), does not draft a
CLA or commercial license/EULA, and does not decide whether or when to
publish — it only documents how, once the owner has decided to.
