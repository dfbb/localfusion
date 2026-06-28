import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Trans } from 'react-i18next'
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
import { type KeyRow } from '../data/schema'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  currentRow: KeyRow
}

export function KeysDeleteDialog({ open, onOpenChange, currentRow }: Props) {
  const qc = useQueryClient()
  const { t } = useTranslation()

  const del = useMutation({
    mutationFn: () => api.delete(`/keys/${currentRow.id}`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['keys'] })
      toast.success(t('common.deleted'))
      onOpenChange(false)
    },
    onError: () => toast.error(t('common.deleteFailed')),
  })

  return (
    <Dialog open={open} onOpenChange={(s) => { if (!del.isPending) onOpenChange(s) }}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="text-destructive">{t('keys.deleteTitle')}</DialogTitle>
          <DialogDescription>
            <Trans
              i18nKey="keys.deleteConfirmBody"
              values={{ label: currentRow.label, irreversible: t('common.irreversible') }}
              components={{ label: <span className="font-semibold" /> }}
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
            onClick={() => del.mutate()}
            disabled={del.isPending}
          >
            {del.isPending ? t('common.deleting') : t('keys.confirmDelete')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
