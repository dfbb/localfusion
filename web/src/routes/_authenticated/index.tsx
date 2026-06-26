import { createFileRoute } from '@tanstack/react-router'
import { Header } from '@/components/layout/header'
import { Main } from '@/components/layout/main'

export const Route = createFileRoute('/_authenticated/')({
  component: Dashboard,
})

function Dashboard() {
  return (
    <>
      <Header fixed>
        <h1 className="text-base font-medium">仪表板</h1>
      </Header>
      <Main>
        <div className="flex flex-col gap-4">
          <h2 className="text-2xl font-bold tracking-tight">欢迎使用 LocalFusion</h2>
          <p className="text-muted-foreground">
            通过左侧导航管理模型、密钥和运维监控。
          </p>
        </div>
      </Main>
    </>
  )
}
