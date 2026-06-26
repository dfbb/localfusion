# P5-T03 虚拟模型页

**阶段:** 5 前端 · **前置:** P5-T01, P5-T02 · 见全局约束: `00-index.md`

**Goal:** `features/virtual-models/` — 列表 + 创建/编辑抽屉（成员上下移排序 + 策略参数动态表单）。对应 `/admin/api/virtual-models` 与 `/admin/api/strategies`（设计 §13.2.2）。

**Files（web/src 下）:** `features/virtual-models/index.tsx`、`features/virtual-models/components/{virtual-models-table,virtual-models-columns,virtual-models-mutate-drawer,virtual-models-provider,virtual-models-primary-buttons,strategy-params-form,member-list}.tsx`、`features/virtual-models/data/schema.ts`、`routes/_authenticated/virtual-models/index.tsx`

**Produces:** 虚拟模型管理页。

- [ ] **Step 1: 查询 strategies + models（供下拉/schema）**

```ts
const { data: strategies } = useQuery({ queryKey:['strategies'], queryFn: () => api.get('/strategies').then(r=>r.data) })
const { data: models } = useQuery({ queryKey:['models'], queryFn: () => api.get('/models').then(r=>r.data) })
const { data: vmodels } = useQuery({ queryKey:['vmodels'], queryFn: () => api.get('/virtual-models').then(r=>r.data) })
```

- [ ] **Step 2: 列表 + 列定义**

`virtual-models-table.tsx`：列 name、strategy（badge）、成员数（`row.members.length`）、row-actions（编辑/删除）。toolbar name 搜索 + strategy faceted-filter。

- [ ] **Step 3: member-list.tsx（成员上下移排序）**

每行：真实模型 Select（options=models）+ 上移/下移按钮（交换数组相邻项）+ 删除；底部「添加成员」。受控 `value: string[]` + `onChange`。failover/speed/cheapest 标注「顺序=优先级」；multimodal 标注「第一行=主推理模型」。

```tsx
function move(arr: string[], i: number, dir: -1 | 1) {
  const j = i + dir; if (j < 0 || j >= arr.length) return arr
  const next = [...arr]; [next[i], next[j]] = [next[j], next[i]]; return next
}
```

- [ ] **Step 4: strategy-params-form.tsx（按 schema 动态渲染）**

输入 `strategyName` → 取 `strategies.find(s=>s.name===strategyName).params_schema.properties` → 遍历渲染控件：`integer`→数字输入、`boolean`→Switch、`enum`→Select、`x-ref:'model'`→真实模型 Select。受控 `value: object` + `onChange`。覆盖设计 §13.2.2 列出的各策略参数（judge/min_answers/strict、timeout_secs、explore/probe_interval_min、tokenizer/output_estimate_max、能力路由+max_iterations）。

- [ ] **Step 5: mutate-drawer.tsx（Sheet 抽屉，组装）**

字段：name（编辑禁用）、strategy（Select）、`<MemberList/>`、`<StrategyParamsForm/>`。提交：
```ts
const save = useMutation({
  mutationFn: (v) => editing ? api.put(`/virtual-models/${v.name}`, v) : api.post('/virtual-models', v),
  onSuccess: () => { qc.invalidateQueries({queryKey:['vmodels']}); toast.success('已保存') },
  onError: (e:any) => toast.error(e.response?.data?.error ?? '保存失败'),
})
// body: { name, strategy, params, members }
```

- [ ] **Step 6: provider + primary-buttons + index + 路由 + 删除**

删除调 `api.delete(/virtual-models/${name})` invalidate。index/provider/primary-buttons/route 同 models 模式。

- [ ] **Step 7: 验证 + 提交**

```bash
cd web && pnpm build
cd .. && git add web/src/features/virtual-models web/src/routes/_authenticated/virtual-models
git commit -m "feat(web): 虚拟模型页(成员上下移 + 策略参数动态表单)"
```
