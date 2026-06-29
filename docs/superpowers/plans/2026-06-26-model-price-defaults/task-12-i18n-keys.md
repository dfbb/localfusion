# Task 12: i18n keys for prices (zh + en)

**Files:**
- Modify: `web/src/i18n/locales/zh.json` (add `models.*` price keys)
- Modify: `web/src/i18n/locales/en.json` (same keys)

**Interfaces:**
- Consumes: the `t('models.*')` keys referenced by Task 11.
- Produces: identical key sets in both locale files; `pnpm check:i18n` passes.

**Context:** The existing `models` block already has keys like `pageTitle`, `maxInputTokensHint`, `notDetected`, `testAll`, etc. Task 11 references six new keys: `priceColumn`, `priceIn`, `priceOut`, `cacheRead`, `cacheWrite`, `priceHint`. Both `zh.json` and `en.json` MUST gain the same keys (the key-parity script `web/scripts/check-i18n-keys.mjs` fails on any mismatch). Chinese is the source of truth; English is the translation. The `models` block is nested under the top-level object; add the keys inside it in both files.

- [ ] **Step 1: Add the six keys to the `models` block in `zh.json`**

In `web/src/i18n/locales/zh.json`, inside the `"models": { ... }` object, add:

```json
    "priceColumn": "价格",
    "priceIn": "输入价格",
    "priceOut": "输出价格",
    "cacheRead": "缓存读取价格",
    "cacheWrite": "缓存写入价格",
    "priceHint": "单位:美元 / 百万 token。添加时留空将按模型名自动匹配默认价格。"
```

- [ ] **Step 2: Add the same six keys to the `models` block in `en.json`**

In `web/src/i18n/locales/en.json`, inside `"models": { ... }`, add:

```json
    "priceColumn": "Price",
    "priceIn": "Input Price",
    "priceOut": "Output Price",
    "cacheRead": "Cache Read Price",
    "cacheWrite": "Cache Write Price",
    "priceHint": "USD per million tokens. Leave blank when adding to auto-match a default by model name."
```

- [ ] **Step 3: Run the key-parity check**

Run from `web/`:
```bash
pnpm check:i18n
```
Expected: `i18n key parity OK (N keys).` (N is the previous count + 6). If it reports a mismatch, the two files don't have identical key sets — fix the typo.

- [ ] **Step 4: Typecheck (JSON imports compile)**

Run from `web/`:
```bash
pnpm exec tsc -b
```
Expected: exit 0.

- [ ] **Step 5: Residue grep (no hardcoded Chinese introduced outside zh.json)**

Run from `web/`:
```bash
grep -rnP '[\x{4e00}-\x{9fff}]' src/features/models --include='*.tsx' --include='*.ts'
```
Expected: no output (all model UI Chinese lives in `zh.json`; Task 11 used `t()` for every label).

- [ ] **Step 6: Commit**

```bash
git add web/src/i18n/locales/zh.json web/src/i18n/locales/en.json
git commit -m "feat(web): add i18n keys for model price fields"
```
