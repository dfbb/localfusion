import { useTranslation } from 'react-i18next'
import { createFileRoute } from '@tanstack/react-router'
import { Header } from '@/components/layout/header'
import { Main } from '@/components/layout/main'
import { Dashboard } from '@/features/dashboard'

export const Route = createFileRoute('/_authenticated/')({
  component: DashboardPage,
})

function DashboardPage() {
  const { t } = useTranslation()
  return (
    <>
      <Header fixed>
        <h1 className="text-base font-medium">{t('nav.dashboardPage')}</h1>
      </Header>
      <Main>
        <Dashboard />
      </Main>
    </>
  )
}
