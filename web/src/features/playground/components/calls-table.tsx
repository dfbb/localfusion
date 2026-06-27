import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Badge } from '@/components/ui/badge'

export type CallRecord = {
  model_id?: string
  role?: string
  input_tokens?: number
  output_tokens?: number
  cost?: number
  status?: string
  estimated?: boolean
  latency_secs?: number
}

type CallsTableProps = {
  calls: CallRecord[]
}

export function CallsTable({ calls }: CallsTableProps) {
  if (!calls || calls.length === 0) {
    return <p className="text-muted-foreground text-sm">无调用记录</p>
  }

  return (
    <div className="rounded-md border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>模型</TableHead>
            <TableHead>角色</TableHead>
            <TableHead className="text-right">Token</TableHead>
            <TableHead className="text-right">费用($)</TableHead>
            <TableHead>状态</TableHead>
            <TableHead className="text-right">延迟(s)</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {calls.map((c, i) => (
            <TableRow key={i}>
              <TableCell className="font-mono text-xs">{c.model_id ?? '-'}</TableCell>
              <TableCell>{c.role ?? '-'}</TableCell>
              <TableCell className="text-right">
                {c.input_tokens != null || c.output_tokens != null
                  ? (c.input_tokens ?? 0) + (c.output_tokens ?? 0)
                  : '-'}
              </TableCell>
              <TableCell className="text-right">
                {c.cost != null ? (
                  <span>
                    {c.cost.toFixed(6)}
                    {c.estimated && (
                      <span className="text-muted-foreground text-xs ml-1">(est)</span>
                    )}
                  </span>
                ) : '-'}
              </TableCell>
              <TableCell>
                {c.status ? (
                  <Badge variant={c.status === 'ok' ? 'default' : 'destructive'}>
                    {c.status}
                  </Badge>
                ) : '-'}
              </TableCell>
              <TableCell className="text-right">
                {c.latency_secs != null ? c.latency_secs.toFixed(3) : '-'}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
