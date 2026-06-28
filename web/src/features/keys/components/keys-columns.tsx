import { type ColumnDef } from '@tanstack/react-table'
import { format } from 'date-fns'
import { MoreHorizontal, Pencil, Shield, Trash2 } from 'lucide-react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { api } from '@/lib/api'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Switch } from '@/components/ui/switch'
import { type KeyRow } from '../data/schema'
import { useKeys } from './keys-provider'

function EnabledSwitch({ row }: { row: { original: KeyRow } }) {
  const qc = useQueryClient()
  const { t } = useTranslation()
  const toggle = useMutation({
    mutationFn: (enabled: boolean) =>
      api.patch(`/keys/${row.original.id}`, { enabled }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['keys'] }),
    onError: () => toast.error(t('common.updateFailed')),
  })

  return (
    <Switch
      checked={row.original.enabled}
      onCheckedChange={(v) => toggle.mutate(v)}
      disabled={toggle.isPending}
    />
  )
}

function RowActions({ row }: { row: { original: KeyRow } }) {
  const { setOpen, setCurrentRow } = useKeys()
  const { t } = useTranslation()
  return (
    <DropdownMenu modal={false}>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" className="h-8 w-8 p-0">
          <MoreHorizontal className="h-4 w-4" />
          <span className="sr-only">{t('common.actions')}</span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-40">
        <DropdownMenuItem
          onClick={() => {
            setCurrentRow(row.original)
            setOpen('acl')
          }}
        >
          <Shield className="mr-2 h-4 w-4" />
          {t('keys.editAcl')}
        </DropdownMenuItem>
        <DropdownMenuItem
          onClick={() => {
            setCurrentRow(row.original)
            setOpen('edit-label')
          }}
        >
          <Pencil className="mr-2 h-4 w-4" />
          {t('keys.editLabel')}
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

export const keysColumns: ColumnDef<KeyRow>[] = [
  {
    accessorKey: 'label',
    header: () => { const { t } = useTranslation(); return t('keys.label') },
    cell: ({ row }) => (
      <span className="font-medium">{row.getValue('label')}</span>
    ),
  },
  {
    accessorKey: 'enabled',
    header: () => { const { t } = useTranslation(); return t('common.status') },
    cell: ({ row }) => <EnabledSwitch row={row} />,
  },
  {
    accessorKey: 'created_at',
    header: () => { const { t } = useTranslation(); return t('common.createdAt') },
    cell: ({ row }) => {
      const ts = row.getValue<number>('created_at')
      return (
        <span className="text-sm text-muted-foreground">
          {format(new Date(ts * 1000), 'yyyy-MM-dd HH:mm')}
        </span>
      )
    },
  },
  {
    accessorKey: 'acl_all',
    header: 'ACL',
    cell: ({ row }) => {
      const { t } = useTranslation()
      const aclAll = row.getValue<boolean>('acl_all')
      return aclAll ? (
        <Badge variant="outline" className="text-green-700 border-green-300">{t('keys.aclAll')}</Badge>
      ) : (
        <Badge variant="outline" className="text-amber-700 border-amber-300">{t('keys.aclWhitelist')}</Badge>
      )
    },
  },
  {
    id: 'actions',
    cell: ({ row }) => <RowActions row={row} />,
  },
]
