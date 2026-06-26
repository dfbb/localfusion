import { Plus } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { useKeys } from './keys-provider'

export function KeysPrimaryButtons() {
  const { setOpen } = useKeys()
  return (
    <Button onClick={() => setOpen('create')}>
      <Plus className="mr-2 h-4 w-4" />
      新建密钥
    </Button>
  )
}
