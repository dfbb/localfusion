import { useTranslation } from 'react-i18next'
import { Header } from '@/components/layout/header'
import { Main } from '@/components/layout/main'
import { VirtualModelsMutateDrawer } from './components/virtual-models-mutate-drawer'
import { VirtualModelsDeleteDialog } from './components/virtual-models-delete-dialog'
import { VirtualModelsPrimaryButtons } from './components/virtual-models-primary-buttons'
import { VirtualModelsProvider, useVirtualModels } from './components/virtual-models-provider'
import { VirtualModelsTable } from './components/virtual-models-table'

function VirtualModelsDialogs() {
  const { open, setOpen, currentRow } = useVirtualModels()
  return (
    <>
      <VirtualModelsMutateDrawer
        open={open === 'add' || open === 'edit'}
        onOpenChange={(s) => { if (!s) setOpen(null) }}
        currentRow={open === 'edit' ? currentRow : null}
      />
      {currentRow && (
        <VirtualModelsDeleteDialog
          open={open === 'delete'}
          onOpenChange={(s) => { if (!s) setOpen(null) }}
          currentRow={currentRow}
        />
      )}
    </>
  )
}

export function VirtualModels() {
  const { t } = useTranslation()
  return (
    <VirtualModelsProvider>
      <Header fixed>
        <h1 className="text-base font-medium">{t('nav.virtualModels')}</h1>
      </Header>

      <Main className="flex flex-1 flex-col gap-4 sm:gap-6">
        <div className="flex flex-wrap items-end justify-between gap-2">
          <div>
            <h2 className="text-2xl font-bold tracking-tight">{t('virtualModels.pageTitle')}</h2>
            <p className="text-muted-foreground">{t('virtualModels.pageSubtitle')}</p>
          </div>
          <VirtualModelsPrimaryButtons />
        </div>
        <VirtualModelsTable />
      </Main>

      <VirtualModelsDialogs />
    </VirtualModelsProvider>
  )
}
