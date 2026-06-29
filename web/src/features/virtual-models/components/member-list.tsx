import { useTranslation } from 'react-i18next'
import { ChevronDown, ChevronUp, Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { type ModelRow } from '@/features/models/data/schema'

function move(arr: string[], i: number, dir: -1 | 1) {
  const j = i + dir
  if (j < 0 || j >= arr.length) return arr
  const next = [...arr]
  ;[next[i], next[j]] = [next[j], next[i]]
  return next
}

type Props = {
  value: string[]
  onChange: (v: string[]) => void
  models: ModelRow[]
  hint?: string
}

export function MemberList({ value, onChange, models, hint }: Props) {
  const { t } = useTranslation()

  function handleChange(i: number, modelId: string) {
    const next = [...value]
    next[i] = modelId
    onChange(next)
  }

  function handleAdd() {
    onChange([...value, ''])
  }

  function handleRemove(i: number) {
    onChange(value.filter((_, idx) => idx !== i))
  }

  return (
    <div className="space-y-2">
      {hint && (
        <p className="text-xs text-muted-foreground">{hint}</p>
      )}
      {value.map((memberId, i) => (
        <div key={memberId || String(i)} className="flex items-center gap-2">
          <span className="w-5 text-xs text-muted-foreground text-right shrink-0">{i + 1}.</span>
          <Select value={memberId} onValueChange={(v) => handleChange(i, v)}>
            <SelectTrigger className="flex-1 h-8">
              <SelectValue placeholder={t('virtualModels.selectModel')} />
            </SelectTrigger>
            <SelectContent>
              {models.map((m) => (
                <SelectItem key={m.id} value={m.id}>
                  <span className="font-mono text-sm">{m.id}</span>
                  <span className="ml-2 text-xs text-muted-foreground">{m.model}</span>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-8 w-8 shrink-0"
            disabled={i === 0}
            onClick={() => onChange(move(value, i, -1))}
          >
            <ChevronUp className="h-4 w-4" />
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-8 w-8 shrink-0"
            disabled={i === value.length - 1}
            onClick={() => onChange(move(value, i, 1))}
          >
            <ChevronDown className="h-4 w-4" />
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-8 w-8 shrink-0 text-destructive hover:text-destructive"
            onClick={() => handleRemove(i)}
          >
            <Trash2 className="h-4 w-4" />
          </Button>
        </div>
      ))}
      <Button type="button" variant="outline" size="sm" className="w-full" onClick={handleAdd}>
        {t('virtualModels.addMember')}
      </Button>
    </div>
  )
}
