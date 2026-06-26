# P5-T01 前端脚手架 + 鉴权 + 布局

**阶段:** 5 前端 · **前置:** P4-T07(管理API) · 见全局约束: `00-index.md`

**Goal:** 在 `web/` 建 Vite + React 19 + TanStack Router/Query + shadcn/ui 脚手架，实现 axios 客户端（token 注入 + 401 跳登录）、zustand 认证 store、登录页、受保护布局（侧边栏 + header）。范式对齐 `../3rd/shadcn-admin`（设计 §13）。

**Files（web/ 下）:** `package.json`、`vite.config.ts`、`tsconfig*.json`、`components.json`、`index.html`、`src/main.tsx`、`src/lib/api.ts`、`src/stores/auth-store.ts`、`src/routes/__root.tsx`、`src/routes/(auth)/sign-in.tsx`、`src/routes/_authenticated/route.tsx`、`src/components/layout/{app-sidebar,header,main}.tsx`、`src/components/layout/data/sidebar-data.ts`、`src/features/auth/sign-in/index.tsx`

**Produces:** 可 `pnpm dev` 启动、登录后进入受保护布局的前端骨架。

- [ ] **Step 1: 初始化 Vite React-TS 工程**

Run:
```bash
cd web && pnpm create vite@latest . --template react-ts
pnpm add @tanstack/react-router @tanstack/react-query @tanstack/react-table axios react-hook-form zod @hookform/resolvers zustand recharts lucide-react sonner date-fns react-day-picker
pnpm add -D @tanstack/router-plugin tailwindcss @tailwindcss/vite
```

- [ ] **Step 2: 配 Tailwind v4 + shadcn**

`vite.config.ts` 加 `@tailwindcss/vite` 与 `@tanstack/router-plugin/vite`（autoCodeSplitting）。`src/styles.css` 引入 `@import "tailwindcss";`。按 shadcn 文档 `pnpm dlx shadcn@latest init` 生成 `components.json` 与基础组件（button/input/card/table/dialog/select/switch/tabs/sonner/sheet/alert-dialog/dropdown-menu/sidebar/badge/checkbox/radio-group）。

- [ ] **Step 3: 写 auth-store.ts**

```ts
import { create } from 'zustand'
const KEY = 'lf_admin_token'
type AuthState = { token: string | null; setToken: (t: string | null) => void }
export const useAuth = create<AuthState>((set) => ({
  token: sessionStorage.getItem(KEY),
  setToken: (t) => { if (t) sessionStorage.setItem(KEY, t); else sessionStorage.removeItem(KEY); set({ token: t }) },
}))
```

- [ ] **Step 4: 写 lib/api.ts（axios 实例 + token 注入 + 401 跳登录）**

```ts
import axios from 'axios'
import { useAuth } from '@/stores/auth-store'
export const api = axios.create({ baseURL: '/admin/api' })
api.interceptors.request.use((cfg) => {
  const t = useAuth.getState().token
  if (t) cfg.headers.Authorization = `Bearer ${t}`
  return cfg
})
api.interceptors.response.use((r) => r, (err) => {
  if (err.response?.status === 401) {
    useAuth.getState().setToken(null)
    window.location.href = '/sign-in'
  }
  return Promise.reject(err)
})
```

- [ ] **Step 5: 写登录页 features/auth/sign-in/index.tsx**

```tsx
import { useState } from 'react'
import { useNavigate } from '@tanstack/react-router'
import axios from 'axios'
import { useAuth } from '@/stores/auth-store'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Card } from '@/components/ui/card'

export function SignIn() {
  const [token, setToken] = useState('')
  const [error, setError] = useState('')
  const setAuth = useAuth((s) => s.setToken)
  const nav = useNavigate()
  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    try {
      await axios.get('/admin/api/health', { headers: { Authorization: `Bearer ${token}` } })
      setAuth(token); nav({ to: '/' })
    } catch { setError('token 无效') }
  }
  return (
    <div className="flex min-h-svh items-center justify-center">
      <Card className="w-80 p-6">
        <form onSubmit={submit} className="space-y-4">
          <h1 className="text-xl font-bold">LocalFusion 管理登录</h1>
          <Input type="password" placeholder="admin token" value={token} onChange={(e) => setToken(e.target.value)} />
          {error && <p className="text-sm text-red-500">{error}</p>}
          <Button type="submit" className="w-full">登录</Button>
        </form>
      </Card>
    </div>
  )
}
```

- [ ] **Step 6: 写路由守卫 + 布局 + 侧边栏数据**

`src/routes/_authenticated/route.tsx` 的 `beforeLoad` 检查 `useAuth.getState().token`，无则 `redirect({to:'/sign-in'})`，并渲染 `<AppSidebar/> + <Header/> + <Outlet/>`。`sidebar-data.ts` 按设计 §13.2 两分组（配置：真实模型/虚拟模型/密钥；运维：监控/调试台/设置）。

- [ ] **Step 7: 验证 + 提交**

```bash
cd web && pnpm build
cd .. && git add web/
git commit -m "feat(web): Vite+TanStack+shadcn 脚手架 + 鉴权 + 受保护布局"
```
