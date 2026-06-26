# P5-T05 监控面板

**阶段:** 5 前端 · **前置:** P5-T01 · 见全局约束: `00-index.md`

**Goal:** `features/dashboard/` — 总用量卡片 + 用量趋势/排行（real/virtual tabs）+ 吞吐延迟 + 价格表 + request_log。对应 `/admin/api/stats/*`（设计 §13.2.4）。

**Files（web/src 下）:** `features/dashboard/index.tsx`、`features/dashboard/components/{summary-cards,usage-chart,usage-ranking,latency-chart,prices-table,requests-table,range-picker}.tsx`、`routes/_authenticated/index.tsx`

**Produces:** 监控面板（`/` 路由）。

- [ ] **Step 1: range-picker + 查询参数**

顶部时间范围（小时/天/周 + 自定义起止，react-day-picker）。维护 `{from, to, granularity}` 状态。粒度=天/周时前端对 hourly 行二次聚合（按 `Math.floor(hour_ts/86400)` 等分桶）。

- [ ] **Step 2: summary-cards.tsx**

```ts
const { data } = useQuery({ queryKey:['usage-summary'], queryFn: ()=>api.get('/stats/usage/summary').then(r=>r.data) })
// 卡片：requests / input_tokens / output_tokens / total_tokens / cost
```
Card 网格（对齐 shadcn-admin dashboard 顶部卡片）。

- [ ] **Step 3: usage-chart.tsx（recharts 折线）**

```ts
const { data } = useQuery({ queryKey:['usage', from, to], queryFn: ()=>
  api.get('/stats/usage', { params:{ scope:'total', from, to } }).then(r=>r.data) })
// 按 granularity 聚合 rows → recharts LineChart：x=时间桶, y=total_tokens / cost(双轴或切换)
```

- [ ] **Step 4: usage-ranking.tsx（real/virtual Tabs + 排行表）**

`Tabs` 切 `scope=real|virtual`，查 `/stats/usage?scope=...&from&to`，前端按 name 聚合求和，TanStack Table 按 total_tokens 降序，列 requests/input/output/total/cost/errors。real tab 列头标注「底层调用数」，virtual tab 标注「请求数」（设计 §8 口径）。

- [ ] **Step 5: latency-chart + prices-table + requests-table**

- `latency-chart`：对每个真实模型查 `/stats/latency?model=` 展示 avg_throughput（v1 简单卡片/条形；趋势图待后端补样本时间序列）。
- `prices-table`：`/stats/prices`，列 model_id/price_in/price_out/updated_at；updated_at 超 7 天标黄。
- `requests-table`：`/stats/requests`，data-table 展示明细（可按 status 前端筛选）。

- [ ] **Step 6: index 组装 + 路由**

`index.tsx`：Header + Main + RangePicker + 卡片 + Tabs(图表/排行) + 延迟 + 价格 + 明细。`routes/_authenticated/index.tsx` 渲染 `<Dashboard/>`。

- [ ] **Step 7: 验证 + 提交**

```bash
cd web && pnpm build
cd .. && git add web/src/features/dashboard web/src/routes/_authenticated/index.tsx
git commit -m "feat(web): 监控面板(总用量/趋势/排行/延迟/价格/明细)"
```
