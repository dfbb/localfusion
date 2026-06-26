import { Label as LabelPrimitive } from 'radix-ui'
import { Switch } from '@/components/ui/switch'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { type ModelRow } from '@/features/models/data/schema'
import { type StrategyParamSchema, type StrategyRow } from '../data/schema'

type Props = {
  strategyName: string
  strategies: StrategyRow[]
  models: ModelRow[]
  value: Record<string, unknown>
  onChange: (v: Record<string, unknown>) => void
}

export function StrategyParamsForm({ strategyName, strategies, models, value, onChange }: Props) {
  const strategy = strategies.find((s) => s.name === strategyName)
  const properties = strategy?.params_schema?.properties

  if (!properties || Object.keys(properties).length === 0) {
    return <p className="text-sm text-muted-foreground">该策略无额外参数。</p>
  }

  function set(key: string, val: unknown) {
    onChange({ ...value, [key]: val })
  }

  return (
    <div className="space-y-3">
      {Object.entries(properties).map(([key, schema]: [string, StrategyParamSchema]) => {
        const cur = value[key]
        const label = key
        const desc = schema.description

        // x-ref: model → model select
        if (schema['x-ref'] === 'model') {
          return (
            <div key={key} className="grid grid-cols-6 items-center gap-x-4 gap-y-1">
              <LabelPrimitive.Root className="col-span-2 text-end text-sm">{label}</LabelPrimitive.Root>
              <div className="col-span-4">
                <Select
                  value={typeof cur === 'string' ? cur : ''}
                  onValueChange={(v) => set(key, v)}
                >
                  <SelectTrigger className="w-full h-8">
                    <SelectValue placeholder="选择模型..." />
                  </SelectTrigger>
                  <SelectContent>
                    {models.map((m) => (
                      <SelectItem key={m.id} value={m.id}>
                        <span className="font-mono text-sm">{m.id}</span>
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                {desc && <p className="text-xs text-muted-foreground mt-0.5">{desc}</p>}
              </div>
            </div>
          )
        }

        // enum → select
        if (schema.enum) {
          return (
            <div key={key} className="grid grid-cols-6 items-center gap-x-4 gap-y-1">
              <LabelPrimitive.Root className="col-span-2 text-end text-sm">{label}</LabelPrimitive.Root>
              <div className="col-span-4">
                <Select
                  value={typeof cur === 'string' ? cur : String(schema.default ?? '')}
                  onValueChange={(v) => set(key, v)}
                >
                  <SelectTrigger className="w-full h-8">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {schema.enum.map((opt) => (
                      <SelectItem key={opt} value={opt}>{opt}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                {desc && <p className="text-xs text-muted-foreground mt-0.5">{desc}</p>}
              </div>
            </div>
          )
        }

        // boolean → Switch
        if (schema.type === 'boolean') {
          return (
            <div key={key} className="grid grid-cols-6 items-center gap-x-4 gap-y-1">
              <LabelPrimitive.Root className="col-span-2 text-end text-sm">{label}</LabelPrimitive.Root>
              <div className="col-span-4 flex items-center gap-2">
                <Switch
                  checked={typeof cur === 'boolean' ? cur : Boolean(schema.default)}
                  onCheckedChange={(v) => set(key, v)}
                />
                {desc && <span className="text-xs text-muted-foreground">{desc}</span>}
              </div>
            </div>
          )
        }

        // integer / number → Input
        if (schema.type === 'integer' || schema.type === 'number') {
          return (
            <div key={key} className="grid grid-cols-6 items-center gap-x-4 gap-y-1">
              <LabelPrimitive.Root className="col-span-2 text-end text-sm">{label}</LabelPrimitive.Root>
              <div className="col-span-4">
                <Input
                  type="number"
                  className="h-8"
                  min={schema.minimum}
                  max={schema.maximum}
                  value={cur !== undefined ? String(cur) : String(schema.default ?? '')}
                  onChange={(e) => {
                    const n = e.target.value === '' ? undefined : Number(e.target.value)
                    set(key, n)
                  }}
                />
                {desc && <p className="text-xs text-muted-foreground mt-0.5">{desc}</p>}
              </div>
            </div>
          )
        }

        // default → text input
        return (
          <div key={key} className="grid grid-cols-6 items-center gap-x-4 gap-y-1">
            <LabelPrimitive.Root className="col-span-2 text-end text-sm">{label}</LabelPrimitive.Root>
            <div className="col-span-4">
              <Input
                className="h-8"
                value={cur !== undefined ? String(cur) : String(schema.default ?? '')}
                onChange={(e) => set(key, e.target.value)}
              />
              {desc && <p className="text-xs text-muted-foreground mt-0.5">{desc}</p>}
            </div>
          </div>
        )
      })}
    </div>
  )
}
