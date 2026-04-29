# examples

Pre-generated IRs you can install directly without running `recon`. Skip the browser setup, see the pipeline work end-to-end in 30 seconds.

## Try it (4 lines)

```bash
cargo install --git https://github.com/crafter-station/webctl webctl
curl -O https://raw.githubusercontent.com/crafter-station/webctl/main/examples/news-ycombinator-com.webctl.json
webctl install ./news-ycombinator-com.webctl.json --dest ~/.cargo/bin
news-ycombinator-com news --json | jq '.items[0:3]'
```

That's it. No clone, no build, no Chromium.

If you want to use it interactively after install:

```bash
news-ycombinator-com --help
news-ycombinator-com news
news-ycombinator-com open 1
```

## Available examples

| IR | Site | Commands | Archetype |
|---|---|---|---|
| `news-ycombinator-com.webctl.json` | [news.ycombinator.com](https://news.ycombinator.com) | 11 (news, ask, jobs, show, newest, threads, user, ...) | `HttpOnly` (server-rendered) |
| `www-sunat-gob-pe.webctl.json` | [www.sunat.gob.pe](https://www.sunat.gob.pe) | tax consultations, RUC lookups | server-rendered, observed but no public API |

## Why these IRs are committed

These are deterministic outputs of `webctl recon`, not source code. Committing them lets anyone install and use the generated CLIs without running the full recon pipeline (which requires a Chromium instance with `--remote-debugging-port=9222` and `agent-browser`).

When you regenerate an IR from your own recon, drop the new file here and PR it.

## What's in an IR

```jsonc
{
  "meta": { "siteName": "...", "displayName": "...", "irVersion": "0.1.0" },
  "provenance": { "technique": "http", "classifierBucket": "HttpOnly" },
  "operations": [
    {
      "commandPath": ["news"],
      "operationKind": "read",
      "transport": { "kind": "http", "endpointIndex": 2 },
      "extractor": { /* CSS selectors and field shape */ }
    }
  ],
  "http": [ /* captured endpoints */ ],
  "ax":   [ /* accessibility tree captures */ ]
}
```

The IR is portable. The same file feeds `webctl emit cli` (Rust shim binary), `webctl emit just-bash` ([just-bash](https://github.com/vercel-labs/just-bash) `ExecutorConfig`), and future emitters (MCP, OpenAPI).
