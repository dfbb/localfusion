# P5-T04 密钥 / ACL 页

**阶段:** 5 前端 · **前置:** P5-T01, P5-T03 · 见全局约束: `00-index.md`

**Goal:** `features/keys/` — 列表 + 新建（明文一次性展示）+ ACL 编辑 + enabled/label PATCH + 删除。对应 `/admin/api/keys*`（设计 §13.2.3）。

**Files（web/src 下）:** `features/keys/index.tsx`、`features/keys/components/{keys-table,keys-columns,keys-create-dialog,keys-result-dialog,keys-acl-dialog,keys-provider,keys-primary-buttons}.tsx`、`routes/_authenticated/keys/index.tsx`

**Produces:** 密钥/ACL 管理页。

- [ ] **Step 1: 列表 + 列定义**

`useQuery(['keys'], ()=>api.get('/keys'))`。列：label、状态（`Switch`，onChange → `api.patch(/keys/${id},{enabled})` invalidate）、创建时间（`new Date(created_at*1000)` 用 date-fns 格式化）、ACL 摘要（`acl_all?'全部':名单前N+…`，名单来自单独查询或行内展开）、row-actions（编辑 ACL / 改 label / 删除）。绝不显示明文。

- [ ] **Step 2: 新建 + 结果对话框**

`keys-create-dialog.tsx`：输入 label → `api.post('/keys',{label})`；成功后把返回的 `{id,key}` 传给 `keys-result-dialog.tsx`：
```tsx
// 一次性展示明文
<AlertDialog open>
  <AlertDialogContent>
    <p className="font-mono break-all">{plaintextKey}</p>
    <Button onClick={()=>navigator.clipboard.writeText(plaintextKey)}>复制</Button>
    <p className="text-red-500 text-sm">关闭后无法再次查看</p>
    <AlertDialogAction onClick={onClose}>我已保存</AlertDialogAction>
  </AlertDialogContent>
</AlertDialog>
```
关闭后 `qc.invalidateQueries(['keys'])`。

- [ ] **Step 3: ACL 编辑对话框 keys-acl-dialog.tsx**

RadioGroup：「允许全部」/「指定白名单」。选指定时 Checkbox 列表（options=vmodels 名）。初始值来自 `api.get(/keys/${id})` 的 acl 信息（或单独 ACL 查询；v1 可在列表行内带 acl_all，名单点开时拉取）。保存：
```ts
api.put(`/keys/${id}/acl`, { acl_all, names })
```
> 后端 `GET /keys` 返回 `acl_all`；具体名单若需展示，前端可调一个轻量约定：ACL 对话框打开时不强制预填名单（v1 允许重设）。如需精确预填，后端可加 `GET /keys/:id/acl`（可选增强，非阻塞）。

- [ ] **Step 4: 改 label / 删除**

改 label → `api.patch(/keys/${id},{label})`；删除 → `AlertDialog` 确认 → `api.delete(/keys/${id})`，均 invalidate。

- [ ] **Step 5: provider + primary-buttons + index + 路由**

同 models 模式。

- [ ] **Step 6: 验证 + 提交**

```bash
cd web && pnpm build
cd .. && git add web/src/features/keys web/src/routes/_authenticated/keys
git commit -m "feat(web): 密钥/ACL 页(明文一次性展示 + ACL + PATCH + 删除)"
```
