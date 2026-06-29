import { useTranslation } from 'react-i18next'
import { Header } from '@/components/layout/header'
import { Main } from '@/components/layout/main'
import { ModelsActionDialog } from './components/models-action-dialog'
import { ModelsDeleteDialog } from './components/models-delete-dialog'
import { ModelsPrimaryButtons } from './components/models-primary-buttons'
import { ModelsProvider, useModels } from './components/models-provider'
import { ModelsTable } from './components/models-table'

function ModelsDialogs() {
  const { open, setOpen, currentRow } = useModels()
  return (
    <>
      <ModelsActionDialog
        open={open === 'add' || open === 'edit'}
        onOpenChange={(s) => { if (!s) setOpen(null) }}
        currentRow={open === 'edit' ? currentRow : null}
      />
      {currentRow && (
        <ModelsDeleteDialog
          open={open === 'delete'}
          onOpenChange={(s) => { if (!s) setOpen(null) }}
          currentRow={currentRow}
        />
      )}
    </>
  )
}

export function Models() {
  const { t } = useTranslation()
  return (
    <ModelsProvider>
      <Header fixed>
        <h1 className="text-base font-medium">{t('nav.models')}</h1>
      </Header>

      <Main className="flex flex-1 flex-col gap-4 sm:gap-6">
        <div className="flex flex-wrap items-end justify-between gap-2">
          <div>
            <h2 className="text-2xl font-bold tracking-tight">{t('models.pageTitle')}</h2>
            <p className="text-muted-foreground">{t('models.pageSubtitle')}</p>
          </div>
          <ModelsPrimaryButtons />
        </div>
        <ModelsTable />
      </Main>

      <ModelsDialogs />
    </ModelsProvider>
  )
}
