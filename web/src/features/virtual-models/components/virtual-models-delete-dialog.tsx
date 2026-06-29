import { useTranslation, Trans } from 'react-i18next'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { api } from '@/lib/api'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { type VirtualModelRow } from '../data/schema'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  currentRow: VirtualModelRow
}

export function VirtualModelsDeleteDialog({ open, onOpenChange, currentRow }: Props) {
  const { t } = useTranslation()
  const qc = useQueryClient()

  const del = useMutation({
    mutationFn: (name: string) => api.delete(`/virtual-models/${name}`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['vmodels'] })
      toast.success(t('common.deleted'))
      onOpenChange(false)
    },
    onError: () => toast.error(t('common.deleteFailed')),
  })

  return (
    <Dialog open={open} onOpenChange={(s) => { if (!del.isPending) onOpenChange(s) }}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="text-destructive">{t('virtualModels.deleteVirtualModel')}</DialogTitle>
          <DialogDescription>
            <Trans
              i18nKey="virtualModels.deleteConfirmBody"
              values={{ name: currentRow.name, irreversible: t('common.irreversible') }}
              components={{ name: <span className="font-mono font-semibold" /> }}
            />
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={del.isPending}
          >
            {t('common.cancel')}
          </Button>
          <Button
            variant="destructive"
            onClick={() => del.mutate(currentRow.name)}
            disabled={del.isPending}
          >
            {del.isPending ? t('common.deleting') : t('virtualModels.confirmDelete')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
