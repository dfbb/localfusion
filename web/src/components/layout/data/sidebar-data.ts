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
      title: 'nav.groupConfig',
      items: [
        {
          title: 'nav.models',
          url: '/models',
          icon: Bot,
        },
        {
          title: 'nav.virtualModels',
          url: '/virtual-models',
          icon: BrainCircuit,
        },
        {
          title: 'nav.keys',
          url: '/keys',
          icon: KeyRound,
        },
      ],
    },
    {
      title: 'nav.groupOps',
      items: [
        {
          title: 'nav.dashboard',
          url: '/',
          icon: Activity,
        },
        {
          title: 'nav.playground',
          url: '/playground',
          icon: Terminal,
        },
        {
          title: 'nav.settings',
          url: '/settings',
          icon: Settings,
        },
      ],
    },
  ],
}
