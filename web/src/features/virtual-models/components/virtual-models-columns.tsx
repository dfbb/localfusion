import { useTranslation } from 'react-i18next'
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
import { type VirtualModelRow } from '../data/schema'
import { useVirtualModels } from './virtual-models-provider'

const strategyColors: Record<string, string> = {
  failover: 'bg-orange-100 text-orange-800 dark:bg-orange-900 dark:text-orange-200',
  speed: 'bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-200',
  cheapest: 'bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200',
  synthesize: 'bg-purple-100 text-purple-800 dark:bg-purple-900 dark:text-purple-200',
  'best-of-n': 'bg-pink-100 text-pink-800 dark:bg-pink-900 dark:text-pink-200',
  multimodal: 'bg-yellow-100 text-yellow-800 dark:bg-yellow-900 dark:text-yellow-200',
}

function RowActions({ row }: { row: { original: VirtualModelRow } }) {
  const { t } = useTranslation()
  const { setOpen, setCurrentRow } = useVirtualModels()
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

export const virtualModelsColumns: ColumnDef<VirtualModelRow>[] = [
  {
    accessorKey: 'name',
    header: () => { const { t } = useTranslation(); return t('common.name') },
    cell: ({ row }) => (
      <span className="font-mono text-sm">{row.getValue('name')}</span>
    ),
    enableHiding: false,
  },
  {
    accessorKey: 'strategy',
    header: () => { const { t } = useTranslation(); return t('virtualModels.strategy') },
    cell: ({ row }) => {
      const strategy = row.getValue<string>('strategy')
      return (
        <Badge variant="outline" className={strategyColors[strategy] ?? ''}>
          {strategy}
        </Badge>
      )
    },
    filterFn: (row, id, value) => value.includes(row.getValue(id)),
  },
  {
    id: 'members_count',
    header: () => { const { t } = useTranslation(); return t('virtualModels.membersCount') },
    cell: ({ row }) => {
      const { t } = useTranslation()
      return (
        <span className="text-sm text-muted-foreground">
          {t('virtualModels.memberCount', { count: row.original.members.length })}
        </span>
      )
    },
  },
  {
    id: 'actions',
    cell: ({ row }) => <RowActions row={row} />,
  },
]
