import { useState } from 'react'
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
import { type KeyCreateResult } from '../data/schema'
import { useKeys } from './keys-provider'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function KeysCreateDialog({ open, onOpenChange }: Props) {
  const [label, setLabel] = useState('')
  const { setOpen, setCreateResult } = useKeys()
  const qc = useQueryClient()

  const create = useMutation({
    mutationFn: (l: string) =>
      api.post('/keys', { label: l }).then((r) => r.data as KeyCreateResult),
    onSuccess: (data) => {
      qc.invalidateQueries({ queryKey: ['keys'] })
      setCreateResult(data)
      setLabel('')
      onOpenChange(false)
      setOpen('result')
    },
    onError: () => toast.error('创建失败'),
  })

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!label.trim()) return
    create.mutate(label.trim())
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(s) => {
        if (!create.isPending) {
          setLabel('')
          onOpenChange(s)
        }
      }}
    >
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>新建密钥</DialogTitle>
          <DialogDescription>输入标签后创建一个新的接入密钥。</DialogDescription>
        </DialogHeader>
        <form id="key-create-form" onSubmit={handleSubmit} className="py-2">
          <Input
            placeholder="标签（如 my-app）"
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            autoFocus
          />
        </form>
        <DialogFooter>
          <Button
            type="submit"
            form="key-create-form"
            disabled={create.isPending || !label.trim()}
          >
            {create.isPending ? '创建中…' : '创建'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
