import { Plus } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { useVirtualModels } from './virtual-models-provider'

export function VirtualModelsPrimaryButtons() {
  const { t } = useTranslation()
  const { setOpen } = useVirtualModels()
  return (
    <Button onClick={() => setOpen('add')}>
      <Plus className="mr-2 h-4 w-4" />
      {t('virtualModels.createVirtualModel')}
    </Button>
  )
}
