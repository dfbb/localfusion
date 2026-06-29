import { useTranslation } from 'react-i18next'
import { Header } from '@/components/layout/header'
import { Main } from '@/components/layout/main'
import { LoggingForm } from './components/logging-form'

export function Settings() {
  const { t } = useTranslation()
  return (
    <>
      <Header fixed>
        <h1 className="text-base font-medium">{t('nav.settings')}</h1>
      </Header>

      <Main className="flex flex-1 flex-col gap-4 sm:gap-6">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">{t('settings.systemSettings')}</h2>
          <p className="text-muted-foreground">{t('settings.pageSubtitle')}</p>
        </div>
        <LoggingForm />
      </Main>
    </>
  )
}
