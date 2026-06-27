import { useState } from 'react'
import { format, subDays, subHours } from 'date-fns'
import { CalendarIcon } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

export type Granularity = 'hour' | 'day' | 'week'

export interface DateRange {
  from: number // Unix 秒
  to: number   // Unix 秒
  granularity: Granularity
}

interface Props {
  value: DateRange
  onChange: (v: DateRange) => void
}

const PRESETS: { label: string; hours: number; granularity: Granularity }[] = [
  { label: '近 24h', hours: 24, granularity: 'hour' },
  { label: '近 7 天', hours: 24 * 7, granularity: 'day' },
  { label: '近 30 天', hours: 24 * 30, granularity: 'week' },
]

const toSecs = (d: Date) => Math.floor(d.getTime() / 1000)

export function RangePicker({ value, onChange }: Props) {
  const [custom, setCustom] = useState(false)

  function applyPreset(hours: number, granularity: Granularity) {
    const to = new Date()
    const from = hours <= 24 ? subHours(to, hours) : subDays(to, hours / 24)
    setCustom(false)
    onChange({
      from: toSecs(from),
      to: toSecs(to),
      granularity,
    })
  }

  function handleCustomFrom(e: React.ChangeEvent<HTMLInputElement>) {
    onChange({ ...value, from: toSecs(new Date(e.target.value)) })
  }

  function handleCustomTo(e: React.ChangeEvent<HTMLInputElement>) {
    onChange({ ...value, to: toSecs(new Date(e.target.value)) })
  }

  function toLocalDatetime(secs: number) {
    const d = new Date(secs * 1000)
    return format(d, "yyyy-MM-dd'T'HH:mm")
  }

  return (
    <div className="flex flex-wrap items-center gap-2">
      {PRESETS.map((p) => (
        <Button
          key={p.label}
          variant="outline"
          size="sm"
          onClick={() => applyPreset(p.hours, p.granularity)}
        >
          {p.label}
        </Button>
      ))}
      <Button
        variant={custom ? 'default' : 'outline'}
        size="sm"
        onClick={() => setCustom((v) => !v)}
      >
        <CalendarIcon className="mr-1 h-3.5 w-3.5" />
        自定义
      </Button>
      {custom && (
        <>
          <Input
            type="datetime-local"
            className="h-8 w-44 text-xs"
            value={toLocalDatetime(value.from)}
            onChange={handleCustomFrom}
          />
          <span className="text-muted-foreground text-sm">–</span>
          <Input
            type="datetime-local"
            className="h-8 w-44 text-xs"
            value={toLocalDatetime(value.to)}
            onChange={handleCustomTo}
          />
        </>
      )}
    </div>
  )
}
