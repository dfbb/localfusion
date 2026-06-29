import { type ColumnDef } from '@tanstack/react-table'
import { MoreHorizontal, Pencil, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { type ModelRow, type Prices } from '../data/schema'
import { useModels } from './models-provider'

function StatusCell({ modelId }: { modelId: string }) {
  const { testing, testResults, testingIds } = useModels()
  const result = testResults.get(modelId)

  // Pending during a bulk test-all (no result yet) or a single-model probe for this row.
  if ((testing || testingIds.has(modelId)) && !result) {
    return <span className="text-muted-foreground text-sm">⋯</span>
  }
  if (!result) {
    return <span className="text-muted-foreground text-sm">—</span>
  }

  const fixed = result.ok && (result.base_url_fixed || result.connector_fixed)

  const label = result.ok
    ? `✓ ${result.latency_ms}ms${fixed ? ' (fixed)' : ''}`
    : `✗ ${result.error.length > 20 ? result.error.slice(0, 20) + '…' : result.error}`

  let tooltipText: string
  if (result.ok) {
    const meta: string[] = [`${result.latency_ms}ms`]
    if (result.max_tokens) meta.push(`max_tokens ${result.max_tokens.toLocaleString()}`)
    if (fixed) {
      const parts: string[] = []
      if (result.connector_fixed) parts.push(`connector → ${result.connector_fixed}`)
      if (result.base_url_fixed) parts.push(`base_url → ${result.base_url_fixed}`)
      tooltipText = `OK — ${meta.join(' · ')} · auto-corrected: ${parts.join(', ')}`
    } else {
      tooltipText = `OK — ${meta.join(' · ')}`
    }
  } else {
    tooltipText = result.error
  }

  const textClass = result.ok
    ? 'text-green-600 dark:text-green-400 text-sm font-mono cursor-default'
    : 'text-red-600 dark:text-red-400 text-sm font-mono cursor-default'

  return (
    <TooltipProvider delayDuration={200}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span className={textClass}>{label}</span>
        </TooltipTrigger>
        <TooltipContent side="top" className="max-w-sm break-all">
          {tooltipText}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}

const connectorColors: Record<string, string> = {
  chat: 'bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-200',
  anthropic: 'bg-purple-100 text-purple-800 dark:bg-purple-900 dark:text-purple-200',
  responses: 'bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200',
}

function RowActions({ row }: { row: { original: ModelRow } }) {
  const { t } = useTranslation()
  const { setOpen, setCurrentRow } = useModels()
  return (
    <DropdownMenu modal={false}>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" className="h-8 w-8 p-0">
          <MoreHorizontal className="h-4 w-4" />
          <span className="sr-only">{t('common.actions')}</span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-36">
        <DropdownMenuItem
          onClick={() => {
            setCurrentRow(row.original)
            setOpen('edit')
          }}
        >
          <Pencil className="mr-2 h-4 w-4" />
          {t('common.edit')}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          variant="destructive"
          onClick={() => {
            setCurrentRow(row.original)
            setOpen('delete')
          }}
        >
          <Trash2 className="mr-2 h-4 w-4" />
          {t('common.delete')}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

export const modelsColumns: ColumnDef<ModelRow>[] = [
  {
    accessorKey: 'id',
    header: 'ID',
    cell: ({ row }) => (
      <span className="font-mono text-sm">{row.getValue('id')}</span>
    ),
    enableHiding: false,
  },
  {
    accessorKey: 'connector',
    header: () => { const { t } = useTranslation(); return t('models.connector') },
    cell: ({ row }) => {
      const connector = row.getValue<string>('connector')
      return (
        <Badge variant="outline" className={connectorColors[connector] ?? ''}>
          {connector}
        </Badge>
      )
    },
    filterFn: (row, id, value) => value.includes(row.getValue(id)),
  },
  {
    accessorKey: 'model',
    header: () => { const { t } = useTranslation(); return t('models.modelName') },
    cell: ({ row }) => (
      <span className="font-mono text-sm">{row.getValue('model')}</span>
    ),
  },
  {
    accessorKey: 'base_url',
    header: 'Base URL',
    cell: ({ row }) => (
      <span className="max-w-48 truncate block text-sm text-muted-foreground">
        {row.getValue('base_url')}
      </span>
    ),
  },
  {
    id: 'key_status',
    header: () => { const { t } = useTranslation(); return t('models.apiKey') },
    cell: ({ row }) => {
      const { t } = useTranslation()
      const { api_key_enc, api_key_env } = row.original
      if (api_key_enc) {
        return <span className="text-sm text-green-600 dark:text-green-400">{t('models.keyEncrypted')}</span>
      }
      if (api_key_env) {
        return <span className="text-sm text-blue-600 dark:text-blue-400">env: {api_key_env}</span>
      }
      return <span className="text-sm text-muted-foreground">{t('models.keyNotSet')}</span>
    },
  },
  {
    id: 'status',
    header: 'Status',
    cell: ({ row }) => <StatusCell modelId={row.original.id} />,
  },
  {
    id: 'price',
    header: () => {
      const { t } = useTranslation()
      return t('models.priceColumn')
    },
    cell: ({ row, table }) => {
      const { t } = useTranslation()
      const meta = table.options.meta as { priceMap?: Map<string, Prices> } | undefined
      const p = meta?.priceMap?.get(row.original.id)
      if (!p) return <span className="text-muted-foreground">—</span>
      return (
        <span
          className="text-sm tabular-nums"
          title={`${t('models.cacheRead')}: ${p.cache_read} / ${t('models.cacheWrite')}: ${p.cache_write}`}
        >
          {p.price_in} / {p.price_out}
        </span>
      )
    },
  },
  {
    id: 'actions',
    cell: ({ row }) => <RowActions row={row} />,
  },
]
