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
import { type ModelRow } from '../data/schema'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  currentRow: ModelRow
}

export function ModelsDeleteDialog({ open, onOpenChange, currentRow }: Props) {
  const qc = useQueryClient()

  const del = useMutation({
    mutationFn: (id: string) => api.delete(`/models/${id}`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['models'] })
      toast.success('已删除')
      onOpenChange(false)
    },
    onError: (e: any) => {
      if (e.response?.status === 409) {
        const refs = e.response.data.references ?? []
        const names = refs
          .map((r: any) => `${r.virtual_name}(${r.ref_kind})`)
          .join(', ')
        toast.error(`被引用，无法删除：${names || '未知引用'}`)
      } else {
        toast.error('删除失败')
      }
    },
  })

  return (
    <Dialog open={open} onOpenChange={(s) => { if (!del.isPending) onOpenChange(s) }}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="text-destructive">删除模型</DialogTitle>
          <DialogDescription>
            确定要删除模型 <span className="font-mono font-semibold">{currentRow.id}</span> 吗？
            此操作不可撤销。
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
            onClick={() => del.mutate(currentRow.id)}
            disabled={del.isPending}
          >
            {del.isPending ? '删除中…' : '确认删除'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
