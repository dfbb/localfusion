import { useState, useEffect } from 'react'
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
import { Input } from '@/components/ui/input'
import { type KeyRow } from '../data/schema'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  currentRow: KeyRow
}

export function KeysEditLabelDialog({ open, onOpenChange, currentRow }: Props) {
  const [label, setLabel] = useState(currentRow.label)
  const qc = useQueryClient()

  useEffect(() => {
    if (open) setLabel(currentRow.label)
  }, [open, currentRow.label])

  const save = useMutation({
    mutationFn: (l: string) => api.patch(`/keys/${currentRow.id}`, { label: l }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['keys'] })
      toast.success('标签已更新')
      onOpenChange(false)
    },
    onError: () => toast.error('更新失败'),
  })

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!label.trim()) return
    save.mutate(label.trim())
  }

  return (
    <Dialog open={open} onOpenChange={(s) => { if (!save.isPending) onOpenChange(s) }}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>修改标签</DialogTitle>
          <DialogDescription>修改密钥 {currentRow.id} 的标签。</DialogDescription>
        </DialogHeader>
        <form id="key-label-form" onSubmit={handleSubmit} className="py-2">
          <Input
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            autoFocus
          />
        </form>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={save.isPending}>
            取消
          </Button>
          <Button type="submit" form="key-label-form" disabled={save.isPending || !label.trim()}>
            {save.isPending ? '保存中…' : '保存'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
