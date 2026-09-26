# Publication Runbook: Making Runlith a Public Repository

> **Owner-executed. Not run by agents.** This runbook documents the steps for
> the repository owner to take the private `fallowlone/Runlith` repository
> public. A coding agent may read this file and may prepare evidence for the
> preconditions below, but must not run the export/push commands in this
> file, create a GitHub repository, or push to one.

## 1. Purpose

Publishing Runlith turns a private, source-available repository into a
public one under [Business Source License 1.1](../../LICENSE) (see
[ADR-0038](../architecture/adr/ADR-0038-product-licensing-bsl.md)). This is a
one-way, high-consequence action: it starts each shipped version's four-year
BSL Change Date clock (ADR-0038 D4) and exposes the tree to the public
internet. Do it deliberately, not as a side effect of another task.

## 2. Preconditions checklist

Do not proceed to Section 3 until every item below is checked.

- [ ] **ADR-0038 is merged to the repository default branch (`main`).** As of
      this writing that merge has not happened; ADR-0038 was ratified on the
      task branch at commit `c040b6d`. Publishing before merge would publish
      a tree still carrying the old PolyForm Shield `LICENSE`, which
      contradicts the owner's ratified decision.
- [ ] **The NVIDIA API keys are revoked.** Four `nvapi-`-prefixed keys were
      committed in `Support/openvidia/proxy_config.json` at commit
      `56b8a68`. Removing the file from the tree (TASK-0154) does **not**
      remove it from history, and this runbook does not rewrite history (see
      Section 4). The keys must be revoked by the owner at the NVIDIA
      provider before publication, regardless of what this runbook does to
      the tree. This is an owner action; an agent must never be asked to
      confirm key validity, since doing so would require reading the secret.
- [ ] **`gitleaks dir` on the export tree, with this repository's
      `.gitleaks.toml`, returns 0 findings.** Run the command in Section 5.2
      against the *exported* tree (not the live history) and confirm a clean
      result before pushing.
- [ ] **No `/Users/...` (or other contributor-machine) absolute paths remain
      in tracked files.** `git grep -n -I "/Users/"` returns nothing.
- [ ] **`CONTRIBUTING.md` and `SECURITY.md` are present** at the repository
      root and describe the current (no external-PR, BSL, private-vuln-report)
      posture.

## 3. Why a new repository with no history

Publishing the *existing* git history, even after removing the stray files
from the working tree, would publish every earlier commit as-is: every
commit before the ADR-0038 merge carries the **PolyForm Shield 1.0.0**
`LICENSE` in effect at that commit, not BSL 1.1, and that grant cannot be
revoked retroactively once published (ADR-0038 D8: "this ADR does not
attempt to retroactively alter any grant"); and commit `56b8a68` contains
the four now-revoked NVIDIA API keys in plaintext, permanently, in that
commit's blob, regardless of what later commits do.

The chosen alternative is to **export the current `main` tree only, with no
history**, into a brand-new repository whose first commit is the export
itself. This publishes exactly one license (BSL 1.1, as merged) and no
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

Create a clean archive of the exact tree that will become the public
repository's first commit:

```sh
git archive --format=tar HEAD | (mkdir -p /tmp/runlith-public-export && tar -x -C /tmp/runlith-public-export)
cd /tmp/runlith-public-export
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

### 4.3 Create the new public GitHub repository and push

Create the new repository through the GitHub UI or `gh repo create`
(public, no README/license/gitignore auto-generated, since the export
already has them), then:

```sh
git remote add origin <new-repository-git-url>
git push -u origin HEAD:main
```

## 5. Verification (run before and after 4.3)

No personal paths:

```sh
git grep -n -I "/Users/"
```

Expected: no output.

Secret scan of the export tree, using this repository's own
`.gitleaks.toml` (it is a root dotfile, already carried into the export):

```sh
gitleaks dir /tmp/runlith-public-export --config /tmp/runlith-public-export/.gitleaks.toml --redact --no-banner
```

Expected: `no leaks found`. If anything is found, stop, do not push, and
investigate before retrying.

## 6. Version / Change Date bookkeeping

BSL 1.1 applies separately to each version, and each version's four-year
Change Date runs from **that version's own first public distribution**
(ADR-0038 D4; "undefined 'version' for Change Date bookkeeping" is an open
question in ADR-0038's Migration impact). Until this runbook is run, no
version has been publicly distributed and no clock has started. Each time
this runbook (or any later re-export) is run for a new version, record, in a
durable location the owner controls:

- the version identifier published (`product/version.toml` at export time);
- the date of first public distribution (today's date, if this is the
  first push of that version);
- the resulting Change Date (four years later).

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
`LICENSING.md`, "Contributions, trademarks and release readiness").

## 9. Non-goals of this runbook

This runbook does not rewrite the private repository's git history, does
not revoke the NVIDIA keys (owner action, outside git), does not draft a
CLA or commercial license/EULA, and does not decide whether or when to
publish — it only documents how, once the owner has decided to.
