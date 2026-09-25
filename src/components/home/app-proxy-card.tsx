import { invoke } from '@tauri-apps/api/core'
import { Alert, Box, Button, Stack, Typography } from '@mui/material'
import { useCallback, useEffect, useState } from 'react'
import { useNavigate } from 'react-router'

type AppProxyStatus = {
  state: string
  proxy_ready: boolean
  effective_rules_version: number | null
  last_startup_error?: string | null
}

export function AppProxyCard() {
  const navigate = useNavigate()
  const [status, setStatus] = useState<AppProxyStatus | null>(null)
  const [error, setError] = useState('')

  const refresh = useCallback(async () => {
    try {
      setStatus(await invoke<AppProxyStatus>('get_app_proxy_status'))
      setError('')
    } catch (reason) {
      setError(String(reason))
    }
  }, [])

  useEffect(() => {
    void refresh()
  }, [refresh])

  return (
    <Box sx={{ p: 2, border: '1px solid', borderColor: 'divider', borderRadius: 2 }}>
      <Stack spacing={1.5}>
        <Typography variant="h6">应用流量</Typography>
        <Typography variant="body2" color="text.secondary">
          {status?.proxy_ready ? `应用代理运行中，规则版本 ${status.effective_rules_version ?? '未知'}` : '应用代理未就绪；进程与 TCP 连接监控可独立使用。'}
        </Typography>
        {(error || status?.last_startup_error) && <Alert severity="error">{error || status?.last_startup_error}</Alert>}
        <Stack direction="row" spacing={1}>
          <Button variant="contained" onClick={() => navigate('/app-traffic')}>管理应用与出口</Button>
          <Button onClick={() => void refresh()}>刷新状态</Button>
        </Stack>
      </Stack>
    </Box>
  )
}
