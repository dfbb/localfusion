import { Plus } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { useVirtualModels } from './virtual-models-provider'

export function VirtualModelsPrimaryButtons() {
  const { setOpen } = useVirtualModels()
  return (
    <Button onClick={() => setOpen('add')}>
      <Plus className="mr-2 h-4 w-4" />
      新建虚拟模型
    </Button>
  )
}
