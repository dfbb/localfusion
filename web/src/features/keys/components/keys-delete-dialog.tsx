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
import { type KeyRow } from '../data/schema'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  currentRow: KeyRow
}

export function KeysDeleteDialog({ open, onOpenChange, currentRow }: Props) {
  const qc = useQueryClient()

  const del = useMutation({
    mutationFn: () => api.delete(`/keys/${currentRow.id}`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['keys'] })
      toast.success('已删除')
      onOpenChange(false)
    },
    onError: () => toast.error('删除失败'),
  })

  return (
    <Dialog open={open} onOpenChange={(s) => { if (!del.isPending) onOpenChange(s) }}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="text-destructive">删除密钥</DialogTitle>
          <DialogDescription>
            确定要删除密钥 <span className="font-semibold">{currentRow.label}</span> 吗？
            此操作不可撤销，使用该密钥的客户端将立即失效。
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={del.isPending}
          >
            取消
          </Button>
          <Button
            variant="destructive"
            onClick={() => del.mutate()}
            disabled={del.isPending}
          >
            {del.isPending ? '删除中…' : '确认删除'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
