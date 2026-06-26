import { createFileRoute } from '@tanstack/react-router'
import { VirtualModels } from '@/features/virtual-models'

export const Route = createFileRoute('/_authenticated/virtual-models/')({
  component: VirtualModels,
})
