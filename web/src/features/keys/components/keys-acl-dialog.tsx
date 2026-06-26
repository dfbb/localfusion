import { useState, useEffect } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { Label as LabelPrimitive } from 'radix-ui'
import { api } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { type KeyRow } from '../data/schema'

type VirtualModel = { id: string; name: string }

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  currentRow: KeyRow
}

export function KeysAclDialog({ open, onOpenChange, currentRow }: Props) {
  const qc = useQueryClient()
  const [aclAll, setAclAll] = useState(true)
  const [selected, setSelected] = useState<string[]>([])

  useEffect(() => {
    if (open) {
      setAclAll(currentRow.acl_all)
      setSelected([])
    }
  }, [open, currentRow])

  const { data: vmodels = [] } = useQuery<VirtualModel[]>({
    queryKey: ['virtual-models'],
    queryFn: () => api.get('/virtual-models').then((r) => r.data),
    enabled: open,
  })

  const save = useMutation({
    mutationFn: () =>
      api.put(`/keys/${currentRow.id}/acl`, {
        acl_all: aclAll,
        names: aclAll ? [] : selected,
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['keys'] })
      toast.success('ACL 已更新')
      onOpenChange(false)
    },
    onError: () => toast.error('更新失败'),
  })

  function toggleModel(name: string) {
    setSelected((prev) =>
      prev.includes(name) ? prev.filter((n) => n !== name) : [...prev, name]
    )
  }

  return (
    <Dialog open={open} onOpenChange={(s) => { if (!save.isPending) onOpenChange(s) }}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>编辑 ACL — {currentRow.label}</DialogTitle>
          <DialogDescription>
            设置此密钥可访问的虚拟模型范围。
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {/* Mode selector as radio-style */}
          <div className="flex flex-col gap-2">
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="radio"
                name="acl-mode"
                checked={aclAll}
                onChange={() => setAclAll(true)}
                className="accent-primary"
              />
              <span className="text-sm">允许访问全部虚拟模型</span>
            </label>
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="radio"
                name="acl-mode"
                checked={!aclAll}
                onChange={() => setAclAll(false)}
                className="accent-primary"
              />
              <span className="text-sm">指定白名单</span>
            </label>
          </div>

          {/* Model list */}
          {!aclAll && (
            <div className="space-y-2 max-h-48 overflow-y-auto rounded border p-3">
              {vmodels.length === 0 ? (
                <p className="text-sm text-muted-foreground">暂无虚拟模型</p>
              ) : (
                vmodels.map((vm) => (
                  <LabelPrimitive.Root
                    key={vm.id}
                    className="flex items-center gap-2 cursor-pointer"
                    htmlFor={`acl-vm-${vm.id}`}
                  >
                    <Checkbox
                      id={`acl-vm-${vm.id}`}
                      checked={selected.includes(vm.name)}
                      onCheckedChange={() => toggleModel(vm.name)}
                    />
                    <span className="font-mono text-sm">{vm.name}</span>
                  </LabelPrimitive.Root>
                ))
              )}
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={save.isPending}>
            取消
          </Button>
          <Button onClick={() => save.mutate()} disabled={save.isPending}>
            {save.isPending ? '保存中…' : '保存'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
