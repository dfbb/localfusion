# P5-T02 真实模型页

**阶段:** 5 前端 · **前置:** P5-T01 · 见全局约束: `00-index.md`

**Goal:** `features/models/` — 列表（TanStack Table）+ 新建/编辑对话框 + 删除（409 引用提示）。对应 `/admin/api/models`（设计 §13.2.1）。

**Files（web/src 下）:** `features/models/index.tsx`、`features/models/components/{models-table,models-columns,models-action-dialog,models-delete-dialog,models-provider,models-primary-buttons}.tsx`、`features/models/data/schema.ts`、`routes/_authenticated/models/index.tsx`

**Produces:** 真实模型管理页。

- [ ] **Step 1: data/schema.ts（zod）**

```ts
import { z } from 'zod'
export const modelSchema = z.object({
  id: z.string().min(1),
  connector: z.enum(['chat', 'anthropic', 'responses']),
  base_url: z.string().url(),
  api_key: z.string().optional(),       // 明文，仅提交用
  api_key_env: z.string().optional(),
  model: z.string().min(1),
  anthropic_version: z.string().optional(),
  extra: z.string().optional(),         // JSON 文本
})
export type ModelForm = z.infer<typeof modelSchema>
export type ModelRow = {
  id: string; connector: string; base_url: string;
  api_key_enc: string | null; api_key_env: string | null;
  model: string; anthropic_version: string | null; extra: string | null
}
```

- [ ] **Step 2: 列表查询 + 表格 + 列定义**

`models-table.tsx` 用 `useQuery({queryKey:['models'], queryFn: () => api.get('/models').then(r=>r.data)})` + TanStack Table；列：id、connector（badge）、base_url、model、密钥状态（`api_key_enc?'已加密存储':api_key_env?`env: ${api_key_env}`:'未配置'`）、row-actions。toolbar 提供 id 搜索 + connector faceted-filter（沿用 shadcn-admin `components/data-table`）。

- [ ] **Step 3: 新建/编辑对话框 models-action-dialog.tsx**

react-hook-form + zodResolver，字段见 schema；`connector==='anthropic'` 时显示 anthropic_version；密钥用 RadioGroup（api_key 直填 / api_key_env）；编辑时 api_key 占位「已设置（留空不变）」。提交：
```ts
const m = useMutation({
  mutationFn: (v: ModelForm) => editing
    ? api.put(`/models/${v.id}`, v) : api.post('/models', v),
  onSuccess: () => { qc.invalidateQueries({queryKey:['models']}); toast.success('已保存') },
})
```

- [ ] **Step 4: 删除对话框（409 引用提示）**

```ts
const del = useMutation({
  mutationFn: (id: string) => api.delete(`/models/${id}`),
  onSuccess: () => { qc.invalidateQueries({queryKey:['models']}); toast.success('已删除') },
  onError: (e: any) => {
    if (e.response?.status === 409) {
      const refs = e.response.data.references ?? []
      toast.error(`被引用，无法删除：${refs.map((r:any)=>`${r.virtual_name}(${r.ref_kind})`).join(', ')}`)
    } else toast.error('删除失败')
  },
})
```

- [ ] **Step 5: provider + primary-buttons + index 组装 + 路由**

`models-provider.tsx`（context 管理 open 对话框 + 选中行）、`models-primary-buttons.tsx`（「新建模型」）、`index.tsx`（Header + Main + Provider + Table + Dialogs）、`routes/_authenticated/models/index.tsx` 渲染 `<Models/>`。

- [ ] **Step 6: 验证 + 提交**

```bash
cd web && pnpm build
cd .. && git add web/src/features/models web/src/routes/_authenticated/models
git commit -m "feat(web): 真实模型页(列表/增改/删除409提示)"
```
