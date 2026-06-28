import { useTranslation } from 'react-i18next'
import { Header } from '@/components/layout/header'
import { Main } from '@/components/layout/main'
import { KeysAclDialog } from './components/keys-acl-dialog'
import { KeysCreateDialog } from './components/keys-create-dialog'
import { KeysDeleteDialog } from './components/keys-delete-dialog'
import { KeysEditLabelDialog } from './components/keys-edit-label-dialog'
import { KeysPrimaryButtons } from './components/keys-primary-buttons'
import { KeysProvider, useKeys } from './components/keys-provider'
import { KeysResultDialog } from './components/keys-result-dialog'
import { KeysTable } from './components/keys-table'

function KeysDialogs() {
  const { open, setOpen, currentRow, createResult, setCreateResult } = useKeys()

  return (
    <>
      <KeysCreateDialog
        open={open === 'create'}
        onOpenChange={(s) => { if (!s) setOpen(null) }}
      />

      {createResult && (
        <KeysResultDialog
          open={open === 'result'}
          result={createResult}
          onClose={() => {
            setCreateResult(null)
            setOpen(null)
          }}
        />
      )}

      {currentRow && (
        <>
          <KeysAclDialog
            open={open === 'acl'}
            onOpenChange={(s) => { if (!s) setOpen(null) }}
            currentRow={currentRow}
          />
          <KeysEditLabelDialog
            open={open === 'edit-label'}
            onOpenChange={(s) => { if (!s) setOpen(null) }}
            currentRow={currentRow}
          />
          <KeysDeleteDialog
            open={open === 'delete'}
            onOpenChange={(s) => { if (!s) setOpen(null) }}
            currentRow={currentRow}
          />
        </>
      )}
    </>
  )
}

export function Keys() {
  const { t } = useTranslation()
  return (
    <KeysProvider>
      <Header fixed>
        <h1 className="text-base font-medium">{t('nav.keys')}</h1>
      </Header>

      <Main className="flex flex-1 flex-col gap-4 sm:gap-6">
        <div className="flex flex-wrap items-end justify-between gap-2">
          <div>
            <h2 className="text-2xl font-bold tracking-tight">{t('keys.pageTitle')}</h2>
            <p className="text-muted-foreground">{t('keys.pageSubtitle')}</p>
          </div>
          <KeysPrimaryButtons />
        </div>
        <KeysTable />
      </Main>

      <KeysDialogs />
    </KeysProvider>
  )
}
