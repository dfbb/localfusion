# Web UI Internationalization (i18n) — Design

**Date:** 2026-06-27
**Status:** Approved

## Overview

The admin web UI is currently hardcoded in Simplified Chinese (~303 string
occurrences across 40 files). Add internationalization so the UI displays a single
language at a time — Chinese or English — chosen automatically from the browser
language on first visit, with a manual switcher and persisted preference.

This is **frontend-only**. Backend-returned error strings are displayed as-is.

---

## Goals

- Single-language switching (the UI shows Chinese OR English, never both at once).
- Auto-detect language from `navigator.language` on first visit; fall back to English.
- Manual language switcher in the top-right header, visible on every page.
- Persist the manual choice in `localStorage`; prefer it on subsequent visits.
- No backend changes.

## Non-Goals (YAGNI)

- Backend i18n / translating backend error messages.
- A third language, translation-management platform, or crowdsourced translation.
- SSR or URL-based locale prefixes (e.g. `/en/models`).
- Introducing a full frontend test framework (a lightweight key-parity check is added instead).

---

## Stack Decision

Use **react-i18next** (`i18next` + `react-i18next` + `i18next-browser-languagedetector`).

Rationale: the requested behavior (browser detection + `localStorage` persistence +
manual switch) is exactly what `i18next-browser-languagedetector` provides out of the
box. Interpolation and pluralization are built in — the codebase already has dynamic
strings (`{count} 个`, `共 {n} 条`, `运行失败: ${error}`). Three dependencies (~40KB
gzip) is acceptable in a project already using axios + the TanStack suite, and adding a
third language later is zero-cost. (Alternatives considered: a zero-dependency custom
Context — rejected because hand-rolling interpolation/plurals across 303 strings is
error-prone; LinguiJS — rejected as over-weight for a 6-page internal admin UI with
Babel/SWC macro integration cost.)

---

## Architecture

### Dependencies (3 new)

- `i18next`
- `react-i18next`
- `i18next-browser-languagedetector`

### Directory layout

```
web/src/i18n/
  index.ts          # i18next initialization + config
  locales/
    zh.json         # Chinese strings (source of truth, extracted from existing code)
    en.json         # English translations
```

### Initialization (`i18n/index.ts`)

- `supportedLngs: ['zh', 'en']`
- `fallbackLng: 'en'` — used when detection yields an unsupported language.
- `load: 'languageOnly'` — normalize region variants (`zh-CN`, `zh-TW`, `en-US`) to
  `zh` / `en`.
- Detection order: `['localStorage', 'navigator']` — a stored manual choice wins;
  otherwise the browser language decides.
- `localStorage` key: `lf_lang` (consistent with the existing `lf_admin_token` naming).
- `detection.caches: ['localStorage']` — the switcher's `changeLanguage` call writes the
  choice back automatically.
- Resources (`zh.json`, `en.json`) are imported statically into the bundle (synchronous
  init), so there is no Suspense/loading state and no first-paint flicker.
- Register a `languageChanged` listener that sets `document.documentElement.lang` to the
  active language (accessibility + correct browser behavior).

### Entry point (`main.tsx`)

- Add `import './i18n'` at the top, before render, so initialization completes first.

---

## String Organization

### Namespace strategy

A single default namespace (`translation`) with flat dot-separated keys grouped by
functional domain. Six pages do not warrant multiple namespaces.

### Key groups (mirror the `features/` directory)

```jsonc
{
  "common": {          // cross-page high-frequency terms
    "save": "保存", "saving": "保存中…", "cancel": "取消",
    "delete": "删除", "deleting": "删除中…", "confirm": "确认",
    "edit": "编辑", "create": "新建", "saveFailed": "保存失败", "totalRows": "..."
    // ...
  },
  "nav": {             // sidebar (sidebar-data.ts)
    "groupConfig": "配置", "groupOps": "运维",
    "models": "真实模型", "virtualModels": "虚拟模型",
    "keys": "密钥 / ACL", "dashboard": "监控",
    "playground": "调试台", "settings": "设置"
  },
  "models": { /* real-models page */ },
  "virtualModels": { /* virtual-models page */ },
  "keys": { /* keys / ACL page */ },
  "dashboard": { /* monitoring page */ },
  "playground": { /* playground page */ },
  "settings": { /* settings page */ }
}
```

### Key naming rule

`<domain>.<semantic>` where the semantic part is camelCase describing *purpose*, not
content (`models.testAll`, not `models.ceshiquanbu`). This keeps keys stable when the
copy text changes.

### Interpolation

i18next double-brace interpolation. The backend-supplied portion of a message is passed
as a variable and is **not** translated:

- `{count} 个` → `t('virtualModels.memberCount', { count })` → `"{{count}} 个成员"` / `"{{count}} members"`
- `共 {n} 条` → `t('common.totalRows', { count: n })`
- `` `运行失败: ${data.error}` `` → `t('playground.runFailed', { error: data.error })`

### Pluralization

Chinese has no plural inflection; English uses i18next's `_one`/`_other` key suffixes,
applied only where there is genuine plural semantics (e.g. "1 member" vs "3 members").
Most counts use plain interpolation.

### Source of truth

`zh.json` holds the existing copy verbatim (extracted first); `en.json` is translated
from it. The two files must have an identical key set.

---

## Language Switcher Component & Data Flow

### Component — `web/src/components/layout/lang-switch.tsx`

- Reuses the existing shadcn `DropdownMenu` (already used by the data tables).
- Trigger: a `ghost` icon button showing the lucide-react `Languages` icon plus the
  active short code (`中` / `EN`).
- Menu items: `简体中文` / `English`, with a check mark on the active one.
- Clicking calls `i18n.changeLanguage('zh' | 'en')`; i18next persists to `localStorage`
  and triggers a global re-render of components using `useTranslation()`.

### Placement — `header.tsx` (single integration point)

Add a fixed right-side region to `header.tsx`:
`<div className="ml-auto flex items-center gap-2">` containing `<LangSwitch />`. Every
page renders `<Header>` already, so all pages gain the switcher by editing one file
(rather than editing 6+ page `index.tsx` files, which risks omissions).

### Data flow

```
User clicks switcher
  → i18n.changeLanguage('en')
     → i18next writes localStorage('lf_lang' = 'en')
     → re-renders components subscribed via useTranslation()
     → languageChanged listener sets document.documentElement.lang = 'en'
  → entire UI switches copy, no page reload
```

---

## Migration Strategy

Convert the ~303 Chinese strings to `t()` calls in domain-sized batches. Each batch is
one `features/` subdirectory plus its `en.json` section, independently verifiable
(switch language, confirm the page renders correctly).

Batch order (by dependency and reuse):

1. **Infrastructure first:** `i18n/index.ts` + skeleton `zh.json`/`en.json` (only
   `common` + `nav`) + `lang-switch.tsx` + `header.tsx` integration + `main.tsx` import.
   After this batch the switcher works and the sidebar is bilingual.
2. **nav / sidebar:** `sidebar-data.ts` is a static exported object, and `t()` is a hook
   that cannot be called at module top level. Resolution: store key strings in the data
   (e.g. `title: 'nav.models'`) and call `t(item.title)` in the component that renders
   the sidebar. This is the only structural change in the migration.
3. **Per domain, in order:** `models` → `virtualModels` → `keys` → `dashboard` →
   `playground` → `settings`. Each domain covers all inline copy and toasts in its
   components.

### Specific handling patterns

- **Static data objects** (sidebar-data): store key, translate at render time (above).
- **Conditional states:** `isPending ? '保存中…' : '保存'` → `t(isPending ? 'common.saving' : 'common.save')`.
- **Interpolation:** `` `运行失败: ${data.error}` `` → `t('playground.runFailed', { error: data.error })`; the backend `error` text is not translated.
- **Backend-error fallback:** `e.response?.data?.error ?? '保存失败'` → `... ?? t('common.saveFailed')` — backend error passes through, the frontend fallback is translated.

---

## Error Handling

- **Missing key:** i18next falls back to rendering the key itself. Enable `debug: true`
  and `saveMissing` in development so missing translations warn in the console.
- **Unrecognized language:** `fallbackLng: 'en'`.
- **Key drift between the two JSON files:** caught by review plus an automated key-parity
  check (below).

---

## Testing

The frontend currently has no test framework. This work does not introduce a full one
(out of scope), but adds a lightweight safeguard:

- **Key-parity check:** a small Node script at `web/scripts/check-i18n-keys.mjs` reads
  `zh.json` and `en.json` and asserts their key sets are identical (no missing
  translations, no extras), exiting non-zero on mismatch. Wired into a `pnpm` script
  (`pnpm check:i18n`), runnable locally or in CI.
- **Manual verification per batch:** after each batch, switch 中/EN and confirm all copy,
  interpolation, and plurals on that page are correct, with no residual hardcoded Chinese
  (scan the directory with `grep -P '[\x{4e00}-\x{9fff}]'`; backend pass-through Chinese
  is exempt).
- **Acceptance criterion:** `grep` over `src` (excluding `i18n/locales/zh.json`) finds no
  residual hardcoded Chinese.

---

## Files Changed

| File | Change |
|---|---|
| `web/package.json` | Add 3 i18n dependencies + key-parity `pnpm` script |
| `web/src/i18n/index.ts` | New — i18next init/config |
| `web/src/i18n/locales/zh.json` | New — Chinese source strings |
| `web/src/i18n/locales/en.json` | New — English translations |
| `web/src/main.tsx` | `import './i18n'` |
| `web/src/components/layout/header.tsx` | Add right-side region hosting `<LangSwitch />` |
| `web/src/components/layout/lang-switch.tsx` | New — language switcher |
| `web/src/components/layout/data/sidebar-data.ts` | Store i18n keys instead of literal titles |
| `web/src/features/**/*.tsx` | Replace inline Chinese with `t()` calls (per-domain batches) |
| `web/scripts/check-i18n-keys.mjs` | New — key-parity check (`pnpm check:i18n`) |
