import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Trans, useTranslation } from 'react-i18next'
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
import { type ModelRow } from '../data/schema'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  currentRow: ModelRow
}

export function ModelsDeleteDialog({ open, onOpenChange, currentRow }: Props) {
  const { t } = useTranslation()
  const qc = useQueryClient()

  const del = useMutation({
    mutationFn: (id: string) => api.delete(`/models/${id}`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['models'] })
      toast.success(t('common.deleted'))
      onOpenChange(false)
    },
    onError: (e: any) => {
      if (e.response?.status === 409) {
        const refs = e.response.data.references ?? []
        const names = refs
          .map((r: any) => `${r.virtual_name}(${r.ref_kind})`)
          .join(', ')
        toast.error(t('models.referencedCannotDelete', { names: names || t('models.unknownRef') }))
      } else {
        toast.error(t('common.deleteFailed'))
      }
    },
  })

  return (
    <Dialog open={open} onOpenChange={(s) => { if (!del.isPending) onOpenChange(s) }}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="text-destructive">{t('models.deleteModel')}</DialogTitle>
          <DialogDescription>
            <Trans
              i18nKey="models.deleteConfirmBody"
              values={{ id: currentRow.id, irreversible: t('common.irreversible') }}
              components={{ id: <span className="font-mono font-semibold" /> }}
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
            onClick={() => del.mutate(currentRow.id)}
            disabled={del.isPending}
          >
            {t(del.isPending ? 'common.deleting' : 'models.confirmDelete')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
