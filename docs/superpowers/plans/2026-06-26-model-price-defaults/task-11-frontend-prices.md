# Task 11: Frontend — price inputs (add/edit), list column, two-call save

**Files:**
- Modify: `web/src/features/models/components/models-action-dialog.tsx` (four price inputs + two-call save)
- Modify: `web/src/features/models/components/models-columns.tsx` (price column)
- Modify: `web/src/features/models/components/models-table.tsx` (fetch + merge prices by model_id)
- Modify: `web/src/features/models/data/schema.ts` (price types on `ModelRow` + a `Prices` type)

**Interfaces:**
- Consumes: `PUT /admin/api/models/:id/prices`, `POST/PUT /admin/api/models`, `GET /admin/api/stats/prices` (Task 10 + existing).
- Produces: prices visible in the table and editable in the dialog. i18n keys are added in Task 12 (this task references `t('models.priceIn')` etc.; Task 12 defines them — run Task 12's parity check before final review, or fold Task 12 in).

**Context:** The dialog uses react-hook-form for model fields and separate local state for `maxInputTokens` (a `useState<number>`). Prices follow the same local-state pattern (NOT the zod `modelSchema`), because add (omit blanks) and edit (all four required, separate PUT) have different semantics than the model fields. Prices are USD/million tokens. The models list query is `['models']` in `models-table.tsx`; a parallel `['prices']` query feeds the merge. `api` is the axios instance from `@/lib/api`.

- [ ] **Step 1: Add price types in `web/src/features/models/data/schema.ts`**

Append:

```ts
export type Prices = {
  model_id: string
  price_in: number
  price_out: number
  cache_read: number
  cache_write: number
  updated_at: number
}
```

- [ ] **Step 2: Fetch + expose prices in `models-table.tsx`**

In `web/src/features/models/components/models-table.tsx`, alongside the existing models query, add a prices query and build a `Map<string, Prices>`, then pass it to the columns/cells. The existing query is:

```ts
  const { data = [], isLoading } = useQuery<ModelRow[]>({
    queryKey: ['models'],
    queryFn: () => api.get('/models').then((r) => r.data),
  })
```

Add after it:

```ts
  const { data: prices = [] } = useQuery<import('../data/schema').Prices[]>({
    queryKey: ['prices'],
    queryFn: () => api.get('/stats/prices').then((r) => r.data),
  })
  const priceMap = new Map(prices.map((p) => [p.model_id, p]))
```

The columns are produced via the existing `modelsColumns` (or a `useModelsColumns` if Task earlier made headers function-based). Pass `priceMap` to the table's `meta` so the price cell can read it:

```ts
  const table = useReactTable({
    data,
    columns: modelsColumns,
    getCoreRowModel: getCoreRowModel(),
    meta: { priceMap },
  })
```

(If `useReactTable` options already exist, add the `meta` field; if a `meta` is already set, add `priceMap` to it. TanStack passes `table.options.meta` to cells via `table.options.meta`.)

- [ ] **Step 3: Add the price column in `models-columns.tsx`**

Add a column (before the `actions` column). It reads the price map from table meta and renders in/out compactly with cache prices in the title tooltip:

```tsx
  {
    id: 'price',
    header: () => {
      const { t } = useTranslation()
      return t('models.priceColumn')
    },
    cell: ({ row, table }) => {
      const { t } = useTranslation()
      const meta = table.options.meta as { priceMap?: Map<string, import('../data/schema').Prices> } | undefined
      const p = meta?.priceMap?.get(row.original.id)
      if (!p) return <span className="text-muted-foreground">—</span>
      return (
        <span
          className="text-sm tabular-nums"
          title={`${t('models.cacheRead')}: ${p.cache_read} / ${t('models.cacheWrite')}: ${p.cache_write}`}
        >
          {p.price_in} / {p.price_out}
        </span>
      )
    },
  },
```

(Add `import { useTranslation } from 'react-i18next'` to `models-columns.tsx` if not already present.)

- [ ] **Step 4: Add four price inputs to the dialog**

In `models-action-dialog.tsx`, add local state near `maxInputTokens`:

```tsx
  // Prices (USD per million tokens). Separate from the RHF model form: add omits blanks,
  // edit requires all four and saves via a dedicated PUT.
  const [priceIn, setPriceIn] = useState<string>('')
  const [priceOut, setPriceOut] = useState<string>('')
  const [cacheRead, setCacheRead] = useState<string>('')
  const [cacheWrite, setCacheWrite] = useState<string>('')
```

In the `useEffect` that resets on open, back-fill from the current row's price (fetched via the `['prices']` query in the table — but the dialog doesn't have it; fetch it here). Add a prices query inside the dialog:

```tsx
  const { data: allPrices } = useQuery<import('../data/schema').Prices[]>({
    queryKey: ['prices'],
    queryFn: () => api.get('/stats/prices').then((r) => r.data),
    enabled: open,
  })
```

In the reset effect, when `currentRow` is set:

```tsx
        const pr = allPrices?.find((p) => p.model_id === currentRow.id)
        setPriceIn(pr ? String(pr.price_in) : '0')
        setPriceOut(pr ? String(pr.price_out) : '0')
        setCacheRead(pr ? String(pr.cache_read) : '0')
        setCacheWrite(pr ? String(pr.cache_write) : '0')
```

and in the `else` (add mode) branch:

```tsx
        setPriceIn(''); setPriceOut(''); setCacheRead(''); setCacheWrite('')
```

Add `allPrices` to the effect's dependency array.

Render the four inputs (place this block right after the max-input-tokens block, before the `isEdit && Max Output Tokens` block):

```tsx
          {/* Prices (USD per million tokens). Add: blank => omitted (backend fuzzy-fills).
              Edit: all four required (full replace via PUT). */}
          <div className="grid grid-cols-6 items-center gap-x-4 gap-y-1">
            <LabelPrimitive.Root className="col-span-2 text-end text-sm font-medium">
              {t('models.priceIn')}
            </LabelPrimitive.Root>
            <Input className="col-span-4" type="number" min={0} step="any"
              value={priceIn} onChange={(e) => setPriceIn(e.target.value)} placeholder="0" />
            <LabelPrimitive.Root className="col-span-2 text-end text-sm font-medium">
              {t('models.priceOut')}
            </LabelPrimitive.Root>
            <Input className="col-span-4" type="number" min={0} step="any"
              value={priceOut} onChange={(e) => setPriceOut(e.target.value)} placeholder="0" />
            <LabelPrimitive.Root className="col-span-2 text-end text-sm font-medium">
              {t('models.cacheRead')}
            </LabelPrimitive.Root>
            <Input className="col-span-4" type="number" min={0} step="any"
              value={cacheRead} onChange={(e) => setCacheRead(e.target.value)} placeholder="0" />
            <LabelPrimitive.Root className="col-span-2 text-end text-sm font-medium">
              {t('models.cacheWrite')}
            </LabelPrimitive.Root>
            <Input className="col-span-4" type="number" min={0} step="any"
              value={cacheWrite} onChange={(e) => setCacheWrite(e.target.value)} placeholder="0" />
            <p className="col-span-4 col-start-3 text-xs text-muted-foreground">
              {t('models.priceHint')}
            </p>
          </div>
```

- [ ] **Step 5: Two-call save (model then prices)**

Replace the existing `m` mutation's `onSubmit`/`mutationFn` flow so that:
- **Add:** build the model payload; attach price fields ONLY when non-blank (blank => omit). One POST; the backend fuzzy-fills omitted prices.
- **Edit:** PUT the model first; on success PUT the prices (all four, parsed as numbers, blank treated as 0 since edit fields are pre-filled). Handle partial failure.

Rewrite the mutation:

```tsx
  // Parse a price input; '' => undefined (omit), else Number (NaN guarded to undefined).
  const num = (s: string): number | undefined => {
    if (s.trim() === '') return undefined
    const n = Number(s)
    return Number.isFinite(n) && n >= 0 ? n : undefined
  }

  const m = useMutation({
    mutationFn: async (v: ModelForm) => {
      if (isEdit) {
        await api.put(`/models/${v.id}`, v)               // 1. model config
        await api.put(`/models/${v.id}/prices`, {          // 2. prices (all four required)
          price_in: num(priceIn) ?? 0,
          price_out: num(priceOut) ?? 0,
          cache_read: num(cacheRead) ?? 0,
          cache_write: num(cacheWrite) ?? 0,
        })
      } else {
        const payload: Record<string, unknown> = { ...v }
        const pin = num(priceIn), pout = num(priceOut), pcr = num(cacheRead), pcw = num(cacheWrite)
        if (pin !== undefined) payload.price_in = pin       // omit blanks => backend fuzzy-fills
        if (pout !== undefined) payload.price_out = pout
        if (pcr !== undefined) payload.cache_read = pcr
        if (pcw !== undefined) payload.cache_write = pcw
        await api.post('/models', payload)
      }
    },
    onSuccess: (_data, v) => {
      qc.invalidateQueries({ queryKey: ['models'] })
      qc.invalidateQueries({ queryKey: ['prices'] })
      toast.success(t('common.saved'))
      onOpenChange(false)
      if (isEdit) runTestOne(v.id)
    },
    onError: () => toast.error(t('common.saveFailed')),
  })
```

Keep the existing `onSubmit(v)` that merges `extra`/strips empty model fields, then `m.mutate(payload)`. The `mutationFn` now reads the price local state directly. (Because the model PUT runs before the price PUT, a price failure never leaves the model unsaved; the `['models']` invalidation reflects the saved model even on a price-PUT error — the error toast prompts a retry.)

- [ ] **Step 6: Verify types + build**

Run from `web/`:
```bash
pnpm exec tsc -b
```
Expected: exit 0. (If `useQuery`/`api` import is missing in any edited file, add it — `import { useQuery } from '@tanstack/react-query'`, `import { api } from '@/lib/api'`.)

- [ ] **Step 7: Manual smoke (note in report, not blocking)**

`pnpm dev`: the models table shows a price column (`in / out`, `—` when unpriced, cache prices in the cell tooltip). Add a model with blank prices → after save the row shows fuzzy-matched defaults. Edit a model, change a price → saved via the prices PUT. Switch zh/EN → labels translate (Task 12 keys).

- [ ] **Step 8: Commit**

```bash
git add web/src/features/models
git commit -m "feat(web): show and edit model prices (input/output/cache) in real-models UI"
```
