import { Button, ButtonGroup } from '@mui/material'
import { useTranslation } from 'react-i18next'

import getSystem from '@/utils/get-system'

type ThemeValue = IVergeConfig['theme_mode']

interface Props {
  value?: ThemeValue
  onChange?: (value: ThemeValue) => void
}

export const ThemeModeSwitch = (props: Props) => {
  const { value, onChange } = props
  const { t } = useTranslation()

  const modes = getSystem() === 'windows'
    ? (['light', 'dark', 'system', 'glass'] as const)
    : (['light', 'dark', 'system'] as const)

  return (
    <ButtonGroup size="small" sx={{ my: '4px' }}>
      {modes.map((mode) => (
        <Button
          key={mode}
          variant={mode === value ? 'contained' : 'outlined'}
          onClick={() => onChange?.(mode)}
          sx={{ textTransform: 'capitalize' }}
        >
          {t(`settings.sections.appearance.${mode}`)}
        </Button>
      ))}
    </ButtonGroup>
  )
}
