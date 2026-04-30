# crosspointreader.com — Catalog API for Xteink Unlocker

**A small addition to the existing Cloudflare Worker to expose available firmware versions to Xteink Unlocker.**

Version 0.3 — Draft

> **Context:** crosspointreader.com already builds, stores, and serves CrossPoint firmware across three channels (stable releases from GitHub Releases, insider nightlies built via GitHub Actions and stored in R2, beta builds uploaded manually and stored in R2). All the producer-side infrastructure exists. Xteink Unlocker just needs a way to enumerate "what's available" so its firmware selection screen can be populated. This spec proposes a single new endpoint that aggregates the three existing sources.

---

## 1. What already exists

The Cloudflare Worker at crosspointreader.com already exposes the data Unlocker needs, just not in one place:

| Endpoint | Returns |
|---|---|
| `GET /api/build/latest` | Latest insider build metadata: commit, version, build date, AI-generated summary, changelog |
| `GET /api/build/firmware` | Latest insider firmware binary from R2 |
| `GET /api/release/latest` | Latest stable release metadata from GitHub Releases |
| `GET /api/release/firmware` | Latest stable firmware binary, proxied from GitHub Releases |
| `GET /api/beta` | List of all beta builds with id, name, notes, created timestamp, size |
| `GET /api/beta/{id}/firmware` | Beta firmware binary by id from R2 |

Unlocker can consume these directly. The only gap is that there's no single endpoint that returns "everything across all channels in one shot" — Unlocker would have to make three requests and stitch them together. Cheap and easy, but worth solving with one aggregator endpoint to keep clients simple.

## 2. The new endpoint

```
GET /api/catalog
```

Returns every currently-available CrossPoint release across all channels in one flat JSON payload. Cached briefly at the edge.

### Response shape

```json
{
  "schema_version": 1,
  "generated_at": "2026-04-29T12:00:00Z",
  "releases": [
    {
      "id": "stable-1.2.0",
      "channel": "stable",
      "name": "1.2.0",
      "version": "1.2.0",
      "released_at": "2026-04-15T00:00:00Z",
      "notes": "Improved EPUB rendering speed\nAdded support for custom sleep screen images\nBug fixes",
      "firmware_url": "https://crosspointreader.com/api/release/firmware",
      "firmware_sha256": "abc123…",
      "size": 6291456
    },
    {
      "id": "insider-a3f8c1d",
      "channel": "insider",
      "name": "master-a3f8c1d",
      "version": "master-a3f8c1d",
      "released_at": "2026-04-29T03:14:00Z",
      "notes": "Improved EPUB rendering speed. Fixed several font loading bugs.",
      "firmware_url": "https://crosspointreader.com/api/build/firmware",
      "firmware_sha256": "def456…",
      "size": 6300000
    },
    {
      "id": "beta-lr3k9p2",
      "channel": "beta",
      "name": "Remote font downloads + SD storage",
      "version": "1.3.0-beta.1",
      "released_at": "2026-04-20T15:30:00Z",
      "notes": "Test build for the upcoming 1.3 features. Known issue: …",
      "firmware_url": "https://crosspointreader.com/api/beta/beta-lr3k9p2/firmware",
      "firmware_sha256": "789abc…",
      "size": 6285000
    },
    {
      "id": "beta-mq7x2k4",
      "channel": "beta",
      "name": "Calibre sync experiment",
      "version": "1.3.0-beta.2",
      "released_at": "2026-04-22T09:15:00Z",
      "notes": "Alternate beta exploring calibre sync. Not compatible with the SD storage beta.",
      "firmware_url": "https://crosspointreader.com/api/beta/beta-mq7x2k4/firmware",
      "firmware_sha256": "bcd012…",
      "size": 6290000
    }
  ]
}
```

A flat array. Every release — stable, insider, or any of the currently-active betas — is one row, distinguished by the `channel` field. Multiple beta entries are common: betas exist for distinct testing purposes (different feature branches, different cohorts), and Unlocker needs to be able to show all of them.

### Field semantics

| Field | Stable | Insider | Beta |
|---|---|---|---|
| `id` | `stable-{tag}` | `insider-{commit-shorthash}` | beta entry's existing id |
| `name` | tag string (`"1.2.0"`) | `master-{shorthash}` | author-supplied name (the differentiator that lets a user pick between active betas) |
| `version` | release tag | `master-{shorthash}` | beta version string if available, else same as `name` |
| `released_at` | GitHub Release `published_at` | build timestamp | beta's `created_at` |
| `notes` | release notes | AI-generated build summary | author-supplied notes |
| `firmware_url` | `/api/release/firmware` | `/api/build/firmware` | `/api/beta/{id}/firmware` |
| `firmware_sha256` | computed on first fetch, cached in KV | computed at upload time | computed at upload time |
| `size` | `Content-Length` from GitHub asset | R2 object size | R2 object size |

`name` is the field Unlocker shows when distinguishing between options *within* a channel. For stable and insider it's typically just the version string; for beta it's the human-readable purpose ("Remote font downloads + SD storage" vs. "Calibre sync experiment").

### Why this shape

- **Symmetric across channels.** Stable, insider, and beta are all rows with the same fields. No special-casing in consumers.
- **Multiple betas just work.** N beta entries, no schema gymnastics. If we ever add a fourth channel (RC, LTS, whatever), it's another value of `channel`, not a schema change.
- **Easy to filter and sort.** `releases.filter(r => r.channel === "beta")` for the beta list. `releases.find(r => r.channel === "stable")` for the current stable.
- **`firmware_url` is absolute.** Unlocker passes it to its downloader without knowing the worker hostname.
- **`firmware_sha256` is canonical from v0.1.** Earlier drafts deferred this to v0.2; promoting it makes integrity verification a hard guarantee from day one.

### Implementation sketch

```ts
case '/api/catalog':
  return handleCatalog(env, corsHeaders);

async function handleCatalog(
  env: Env,
  headers: Record<string, string>
): Promise<Response> {
  const [stable, insider, betaList] = await Promise.all([
    fetchStableForCatalog(env),
    fetchInsiderForCatalog(env),
    getBetaList(env),
  ]);

  const releases = [];
  if (stable) releases.push(stable);
  if (insider) releases.push(insider);
  for (const b of betaList) releases.push(b);

  return json({
    schema_version: 1,
    generated_at: new Date().toISOString(),
    releases,
  }, 200, {
    ...headers,
    'Cache-Control': 'public, max-age=300',
  });
}
```

Each `fetchForCatalog` helper returns the unified row shape. If a channel has nothing available (no insider yet, GitHub down), it returns `null` and is simply omitted from `releases`. Unlocker handles missing channels gracefully (the corresponding card shows "no releases on this channel right now").

### Caching

`Cache-Control: public, max-age=300` — five minutes. Long enough to absorb the "every Unlocker user opens the app at 9am" thundering herd, short enough that a new release or beta is visible within a few minutes of being published.

The existing `/api/release/latest` hits the GitHub API on every request without caching, which is fine for now but would benefit from the same edge cache once `/api/catalog` is the primary read path.

## 3. What about the WebSerial flasher?

The existing flasher already loads metadata via the per-channel endpoints (the "Loading..." placeholders on the homepage). It doesn't need to change. If at some point it makes sense to consolidate the flasher onto `/api/catalog` too, that's a small refactor — but it's not on the critical path for shipping Unlocker.

The flasher serves a different set of needs anyway: it has stock firmware options (English/Chinese) that aren't CrossPoint releases and don't belong in the catalog. Those continue to be served by `/api/firmware/stock` as today.

## 4. Channel semantics in Unlocker

Unlocker's firmware-selection screen shows three cards: **Stable**, **Beta**, **Insider**.

- **Stable** — one tap installs the latest stable release. `notes` are the GitHub release notes.
- **Insider** — one tap installs the latest nightly build. `notes` are the AI-generated build summary. Strong warning copy ("auto-built from master, may be unstable").
- **Beta** — tap behaviour depends on how many betas are active:
  - Zero active betas: card is disabled with "no betas right now".
  - One active beta: tap installs that beta directly.
  - Two or more active betas: tap expands the card into a sub-list, each entry shown by `name` + `notes`. User picks one.

The flat catalog shape makes this trivial: `releases.filter(r => r.channel === "beta")` returns the list to render.

Unlocker isn't gated behind Royalty — it serves users who have no other path to install CrossPoint. The `/api/catalog` endpoint should return insider builds without subscription verification. If gating is needed in the future, it's a separate decision.

## 5. Discovery and risk

**Open design questions for future iterations** (not blockers for v0.1):

- **Per-release minimum-stock-version.** If a future CrossPoint release requires a specific stock firmware floor (e.g., "CrossPoint 1.5 only installs over stock V5.0+"), the catalog would need a per-release `min_stock_version` field. Not relevant today; design for it when it becomes real.
- **Per-release device support flags.** Currently CrossPoint runs on both X3 and X4 from the same binary. If that ever changes, the catalog needs a `supported_devices` field per release.
- **Cohort filtering for betas.** If betas need to be visible only to specific users (paying subscribers, particular hardware revisions), the catalog needs a way to express that. Could be a `visibility` field or a separate authenticated endpoint.

**Risk:** the existing endpoints take real load already (the homepage flasher hits them on every page load), so adding `/api/catalog` doesn't change the infrastructure footprint meaningfully. The main risk is the GitHub API rate limit when stable metadata is uncached and traffic spikes — solved by edge caching `/api/catalog` (5-minute `Cache-Control`) and persisting per-release SHA-256 hashes in KV so the worker isn't re-fetching the GitHub asset on every request.

## 6. Roadmap

**v0.1 — Add `/api/catalog` with the flat shape and SHA-256 baked in.**

- New case in the API switch statement
- Reuses existing internal helpers — refactor the inner logic of `handleLatestRelease`, `handleLatestBuild`, and the beta listing so each can return a unified row shape consumable by both their original endpoints and the aggregator
- Compute and store firmware SHA-256:
  - Insider + beta: hash on the way through during upload (`handleBuildUpload` / `handleBetaCreate` already stream the bytes), persist in R2 `customMetadata`
  - Stable: fetch the GitHub asset on first catalog access, hash it, cache the result in KV keyed by release tag
- Edge cache `/api/catalog` for 5 minutes
- Add edge cache to `/api/release/latest` while in the file

**v0.2 — Schema-additive, only if needed.**

- Per-release `min_stock_version` field
- Per-release `supported_devices` field
- Beta `visibility` / cohort filtering

Schema-additive changes don't bump `schema_version`. A breaking change to the response shape would.

Most of v0.1 is one Cloudflare Worker PR. Maybe a half-day of work, including tests against the existing `wrangler dev` setup.

---

*End of spec, v0.3.*
