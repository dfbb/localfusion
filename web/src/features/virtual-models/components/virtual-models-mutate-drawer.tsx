import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
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

export function VirtualModelsMutateDrawer({ open, onOpenChange, currentRow }: Props) {
  const { t } = useTranslation()
  const isEdit = !!currentRow
  const qc = useQueryClient()

  const [name, setName] = useState('')
  const [strategy, setStrategy] = useState('')
  const [members, setMembers] = useState<string[]>([''])
  const [params, setParams] = useState<Record<string, unknown>>({})

  const strategyHints: Record<string, string> = {
    failover: t('virtualModels.strategyHintFailover'),
    speed: t('virtualModels.strategyHintSpeed'),
    cheapest: t('virtualModels.strategyHintCheapest'),
    multimodal: t('virtualModels.strategyHintMultimodal'),
  }

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


  const save = useMutation({
    mutationFn: (v: { name: string; strategy: string; members: string[]; params: Record<string, unknown> }) =>
      isEdit
        ? api.put(`/virtual-models/${v.name}`, v)
        : api.post('/virtual-models', v),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['vmodels'] })
      toast.success(t('common.saved'))
      onOpenChange(false)
    },
    onError: (e: any) => toast.error(e.response?.data?.error ?? t('common.saveFailed')),
  })

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    const filteredMembers = members.filter(Boolean)
    if (!name) { toast.error(t('virtualModels.nameRequired')); return }
    if (!strategy) { toast.error(t('virtualModels.strategyRequired')); return }
    if (filteredMembers.length === 0) { toast.error(t('virtualModels.membersRequired')); return }
    save.mutate({ name, strategy, members: filteredMembers, params })
  }

  const hint = strategyHints[strategy]

  return (
    <Sheet open={open} onOpenChange={(s) => { if (!save.isPending) onOpenChange(s) }}>
      <SheetContent className="sm:max-w-lg overflow-y-auto">
        <SheetHeader>
          <SheetTitle>{isEdit ? t('virtualModels.editVirtualModel') : t('virtualModels.createVirtualModel')}</SheetTitle>
          <SheetDescription>
            {isEdit ? t('virtualModels.editDescription') : t('virtualModels.createDescription')}
          </SheetDescription>
        </SheetHeader>

        <form id="vmodel-form" onSubmit={handleSubmit} className="space-y-5 py-4 px-1">
          {/* Name */}
          <div className="grid grid-cols-6 items-center gap-x-4 gap-y-1">
            <LabelPrimitive.Root className="col-span-2 text-end text-sm font-medium" htmlFor="vm-name">
              {t('common.name')}
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
              {t('virtualModels.strategy')}
            </LabelPrimitive.Root>
            <div className="col-span-4">
              <Select value={strategy} onValueChange={(v) => { setStrategy(v); if (!isEdit) setParams({}) }}>
                <SelectTrigger className="w-full h-8">
                  <SelectValue placeholder={t('virtualModels.selectStrategy')} />
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
            <LabelPrimitive.Root className="text-sm font-medium">{t('virtualModels.membersLabel')}</LabelPrimitive.Root>
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
              <LabelPrimitive.Root className="text-sm font-medium">{t('virtualModels.strategyParams')}</LabelPrimitive.Root>
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
            {save.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  )
}
