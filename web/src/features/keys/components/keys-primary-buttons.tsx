import { Plus } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { useKeys } from './keys-provider'

export function KeysPrimaryButtons() {
  const { setOpen } = useKeys()
  const { t } = useTranslation()
  return (
    <Button onClick={() => setOpen('create')}>
      <Plus className="mr-2 h-4 w-4" />
      {t('keys.createKey')}
    </Button>
  )
}
