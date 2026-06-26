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
  return (
    <AlertDialog open={open}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>密钥已创建</AlertDialogTitle>
          <AlertDialogDescription>
            请立即复制并保存，关闭后无法再次查看。
          </AlertDialogDescription>
        </AlertDialogHeader>

        <div className="space-y-3">
          <p className="font-mono break-all rounded bg-muted px-3 py-2 text-sm">
            {result.key}
          </p>
          <p className="text-red-500 text-sm font-medium">⚠ 关闭后无法再次查看</p>
          <Button
            variant="outline"
            size="sm"
            className="w-full"
            onClick={() => {
              navigator.clipboard.writeText(result.key)
            }}
          >
            复制到剪贴板
          </Button>
        </div>

        <AlertDialogFooter>
          <AlertDialogAction onClick={onClose}>我已保存</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
