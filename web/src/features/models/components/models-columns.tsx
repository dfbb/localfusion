import { type ColumnDef } from '@tanstack/react-table'
import { MoreHorizontal, Pencil, Trash2 } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { type ModelRow } from '../data/schema'
import { useModels } from './models-provider'

function StatusCell({ modelId }: { modelId: string }) {
  const { testing, testResults } = useModels()
  const result = testResults.get(modelId)

  if (testing && !result) {
    return <span className="text-muted-foreground text-sm">⋯</span>
  }
  if (!result) {
    return <span className="text-muted-foreground text-sm">—</span>
  }
  if (result.ok) {
    return (
      <span className="text-green-600 dark:text-green-400 text-sm font-mono">
        ✓ {result.latency_ms}ms
      </span>
    )
  }
  const short = result.error.length > 12
    ? result.error.slice(0, 12) + '…'
    : result.error
  return (
    <span
      className="text-red-600 dark:text-red-400 text-sm font-mono cursor-default"
      title={result.error}
    >
      ✗ {short}
    </span>
  )
}

const connectorColors: Record<string, string> = {
  chat: 'bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-200',
  anthropic: 'bg-purple-100 text-purple-800 dark:bg-purple-900 dark:text-purple-200',
  responses: 'bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200',
}

function RowActions({ row }: { row: { original: ModelRow } }) {
  const { setOpen, setCurrentRow } = useModels()
  return (
    <DropdownMenu modal={false}>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" className="h-8 w-8 p-0">
          <MoreHorizontal className="h-4 w-4" />
          <span className="sr-only">操作</span>
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
          编辑
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
          删除
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
    header: '连接器',
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
    header: '模型',
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
    header: '密钥',
    cell: ({ row }) => {
      const { api_key_enc, api_key_env } = row.original
      if (api_key_enc) {
        return <span className="text-sm text-green-600 dark:text-green-400">已加密存储</span>
      }
      if (api_key_env) {
        return <span className="text-sm text-blue-600 dark:text-blue-400">env: {api_key_env}</span>
      }
      return <span className="text-sm text-muted-foreground">未配置</span>
    },
  },
  {
    id: 'status',
    header: 'Status',
    cell: ({ row }) => <StatusCell modelId={row.original.id} />,
  },
  {
    id: 'actions',
    cell: ({ row }) => <RowActions row={row} />,
  },
]
