import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { Label as LabelPrimitive } from 'radix-ui'
import { api } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import { type ModelRow } from '@/features/models/data/schema'
import { type VirtualModelRow, type StrategyRow } from '../data/schema'
import { MemberList } from './member-list'
import { StrategyParamsForm } from './strategy-params-form'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  currentRow?: VirtualModelRow | null
}

const STRATEGY_HINTS: Record<string, string> = {
  failover: '顺序=优先级，首个可用模型优先',
  speed: '顺序=优先级，按延迟自动排序',
  cheapest: '顺序=优先级，按价格选最低',
  multimodal: '第一行=主推理模型',
}

export function VirtualModelsMutateDrawer({ open, onOpenChange, currentRow }: Props) {
  const isEdit = !!currentRow
  const qc = useQueryClient()

  const [name, setName] = useState('')
  const [strategy, setStrategy] = useState('')
  const [members, setMembers] = useState<string[]>([''])
  const [params, setParams] = useState<Record<string, unknown>>({})

  const { data: strategies = [] } = useQuery<StrategyRow[]>({
    queryKey: ['strategies'],
    queryFn: () => api.get('/strategies').then((r) => r.data),
  })

  const { data: models = [] } = useQuery<ModelRow[]>({
    queryKey: ['models'],
    queryFn: () => api.get('/models').then((r) => r.data),
  })

  useEffect(() => {
    if (open) {
      if (currentRow) {
        setName(currentRow.name)
        setStrategy(currentRow.strategy)
        setMembers(currentRow.members.length ? currentRow.members : [''])
        setParams(currentRow.params ?? {})
      } else {
        setName('')
        setStrategy('')
        setMembers([''])
        setParams({})
      }
    }
  }, [open, currentRow])

  // Reset params when strategy changes
  useEffect(() => {
    if (!isEdit) setParams({})
  }, [strategy, isEdit])

  const save = useMutation({
    mutationFn: (v: { name: string; strategy: string; members: string[]; params: Record<string, unknown> }) =>
      isEdit
        ? api.put(`/virtual-models/${v.name}`, v)
        : api.post('/virtual-models', v),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['vmodels'] })
      toast.success('已保存')
      onOpenChange(false)
    },
    onError: (e: any) => toast.error(e.response?.data?.error ?? '保存失败'),
  })

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    const filteredMembers = members.filter(Boolean)
    if (!name) { toast.error('请填写名称'); return }
    if (!strategy) { toast.error('请选择策略'); return }
    if (filteredMembers.length === 0) { toast.error('至少需要一个成员'); return }
    save.mutate({ name, strategy, members: filteredMembers, params })
  }

  const hint = STRATEGY_HINTS[strategy]

  return (
    <Sheet open={open} onOpenChange={(s) => { if (!save.isPending) onOpenChange(s) }}>
      <SheetContent className="sm:max-w-lg overflow-y-auto">
        <SheetHeader>
          <SheetTitle>{isEdit ? '编辑虚拟模型' : '新建虚拟模型'}</SheetTitle>
          <SheetDescription>
            {isEdit ? '修改虚拟模型配置。' : '配置新的虚拟模型。'}
            完成后点击保存。
          </SheetDescription>
        </SheetHeader>

        <form id="vmodel-form" onSubmit={handleSubmit} className="space-y-5 py-4 px-1">
          {/* Name */}
          <div className="grid grid-cols-6 items-center gap-x-4 gap-y-1">
            <LabelPrimitive.Root className="col-span-2 text-end text-sm font-medium" htmlFor="vm-name">
              名称
            </LabelPrimitive.Root>
            <Input
              id="vm-name"
              className="col-span-4 h-8"
              placeholder="my-virtual-model"
              disabled={isEdit}
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>

          {/* Strategy */}
          <div className="grid grid-cols-6 items-center gap-x-4 gap-y-1">
            <LabelPrimitive.Root className="col-span-2 text-end text-sm font-medium">
              策略
            </LabelPrimitive.Root>
            <div className="col-span-4">
              <Select value={strategy} onValueChange={setStrategy}>
                <SelectTrigger className="w-full h-8">
                  <SelectValue placeholder="选择策略..." />
                </SelectTrigger>
                <SelectContent>
                  {strategies.map((s) => (
                    <SelectItem key={s.name} value={s.name}>
                      {s.name}
                      {s.description && (
                        <span className="ml-2 text-xs text-muted-foreground">{s.description}</span>
                      )}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          {/* Members */}
          <div className="space-y-2">
            <LabelPrimitive.Root className="text-sm font-medium">成员模型</LabelPrimitive.Root>
            <MemberList
              value={members}
              onChange={setMembers}
              models={models}
              hint={hint}
            />
          </div>

          {/* Strategy Params */}
          {strategy && (
            <div className="space-y-2">
              <LabelPrimitive.Root className="text-sm font-medium">策略参数</LabelPrimitive.Root>
              <StrategyParamsForm
                strategyName={strategy}
                strategies={strategies}
                models={models}
                value={params}
                onChange={setParams}
              />
            </div>
          )}
        </form>

        <SheetFooter className="px-1">
          <Button
            type="submit"
            form="vmodel-form"
            disabled={save.isPending}
          >
            {save.isPending ? '保存中…' : '保存'}
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  )
}
