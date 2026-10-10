---
description: writing locale stubs for a way — the locales.jsonl file of per-language description and vocabulary aliases that let a way match prompts in another language
vocabulary: locales.jsonl locale stub alias per-language native translate translation lang jsonl coordinate multilingual per-locale english-only confuser re-author
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Locale Stubs

Ways can have native-language matching stubs stored in `{wayname}.locales.jsonl` alongside the way file. These are **coordinate aliases** on the way's graph node (ADR-125): one line per language with `description` and `vocabulary` in the target language. The way body stays English. Every alias must carry the objective match words of the way's intent in local form — translations must actually translate, not just share a name.

```jsonl
{"lang":"ja","description":"セキュリティ脆弱性スキャン","vocabulary":"セキュリティ 脆弱性 CVE"}
```

No per-locale threshold field — and none per-node either. Firing is the global calibrated gate (`g(s)` against `τ_s` / `τ_k`) for every alias, English or localized; locale stubs correctly carry no threshold.

**Audit your stubs** with `ways tune locale` — it measures fidelity (do sibling translations agree?) and discrimination (does another way's alias outrank yours?). Entries where a non-sibling confuser wins need the stub re-authored with sharper vocabulary. See `knowledge/optimization/tuning(meta)` for the full workflow and failure-mode categories. Full guide: `docs/hooks-and-ways/languages.md`.

## See Also

- knowledge/authoring(meta) — parent: way format; ways are authored English-only (ADR-139)
- knowledge/optimization/tuning(meta) — locale alias audit, failure modes, re-authoring guidance
