import {
  Bot,
  BrainCircuit,
  KeyRound,
  Activity,
  Terminal,
  Settings,
  Command,
} from 'lucide-react'

export type NavItem = {
  title: string
  url: string
  icon?: React.ComponentType<{ className?: string }>
  badge?: string
}

export type NavGroup = {
  title: string
  items: NavItem[]
}

export type SidebarData = {
  appName: string
  appLogo: React.ComponentType<{ className?: string }>
  navGroups: NavGroup[]
}

export const sidebarData: SidebarData = {
  appName: 'LocalFusion',
  appLogo: Command,
  navGroups: [
    {
      title: '配置',
      items: [
        {
          title: '真实模型',
          url: '/models',
          icon: Bot,
        },
        {
          title: '虚拟模型',
          url: '/virtual-models',
          icon: BrainCircuit,
        },
        {
          title: '密钥 / ACL',
          url: '/keys',
          icon: KeyRound,
        },
      ],
    },
    {
      title: '运维',
      items: [
        {
          title: '监控',
          url: '/',
          icon: Activity,
        },
        {
          title: '调试台',
          url: '/playground',
          icon: Terminal,
        },
        {
          title: '设置',
          url: '/settings',
          icon: Settings,
        },
      ],
    },
  ],
}
