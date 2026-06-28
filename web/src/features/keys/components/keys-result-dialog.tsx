import { useTranslation } from 'react-i18next'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { type KeyCreateResult } from '../data/schema'

type Props = {
  open: boolean
  result: KeyCreateResult
  onClose: () => void
}

export function KeysResultDialog({ open, result, onClose }: Props) {
  const { t } = useTranslation()
  return (
    <AlertDialog open={open}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('keys.resultTitle')}</AlertDialogTitle>
          <AlertDialogDescription>
            {t('keys.resultDesc')}
          </AlertDialogDescription>
        </AlertDialogHeader>

        <div className="space-y-3">
          <p className="font-mono break-all rounded bg-muted px-3 py-2 text-sm">
            {result.key}
          </p>
          <p className="text-red-500 text-sm font-medium">{t('keys.resultWarning')}</p>
          <Button
            variant="outline"
            size="sm"
            className="w-full"
            onClick={() => {
              navigator.clipboard.writeText(result.key)
            }}
          >
            {t('keys.copyToClipboard')}
          </Button>
        </div>

        <AlertDialogFooter>
          <AlertDialogAction onClick={onClose}>{t('keys.resultConfirm')}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
