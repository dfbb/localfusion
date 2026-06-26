import { createFileRoute } from '@tanstack/react-router'
import { Header } from '@/components/layout/header'
import { Main } from '@/components/layout/main'
import { Dashboard } from '@/features/dashboard'

export const Route = createFileRoute('/_authenticated/')({
  component: DashboardPage,
})

function DashboardPage() {
  return (
    <>
      <Header fixed>
        <h1 className="text-base font-medium">监控面板</h1>
      </Header>
      <Main>
        <Dashboard />
      </Main>
    </>
  )
}
