import { Check, Languages } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

const LANGS = [
  { code: 'zh', label: '简体中文', short: '中' },
  { code: 'en', label: 'English', short: 'EN' },
] as const

export function LangSwitch() {
  const { i18n } = useTranslation()
  const active = i18n.resolvedLanguage === 'zh' ? 'zh' : 'en'
  const short = LANGS.find((l) => l.code === active)?.short ?? 'EN'

  return (
    <DropdownMenu modal={false}>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm" className="gap-1.5" aria-label="Switch language">
          <Languages className="h-4 w-4" />
          <span className="text-xs">{short}</span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-36">
        {LANGS.map((l) => (
          <DropdownMenuItem key={l.code} onClick={() => i18n.changeLanguage(l.code)}>
            <Check className={l.code === active ? 'mr-2 h-4 w-4 opacity-100' : 'mr-2 h-4 w-4 opacity-0'} />
            {l.label}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
