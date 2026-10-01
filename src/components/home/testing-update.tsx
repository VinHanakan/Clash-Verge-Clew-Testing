import { invoke } from '@tauri-apps/api/core'
import { Alert, Box, Button, LinearProgress, Typography } from '@mui/material'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

interface UpdateState {
  phase: string
  version?: string | null
  received: number
  total?: number | null
  path?: string | null
  error?: string | null
}

let checkedAt = 0

export const TestingUpdate = () => {
  const { i18n } = useTranslation()
  const zh = i18n.language.startsWith('zh')
  const [state, setState] = useState<UpdateState | null>(null)
  const [busy, setBusy] = useState(false)
  const check = async () => {
    setBusy(true)
    checkedAt = Date.now()
    try {
      const result = await invoke<UpdateState>('check_testing_update')
      setState(result)
      if (result.phase === 'available') {
        setState({ ...result, phase: 'downloading' })
        setState(await invoke<UpdateState>('download_testing_update'))
      }
    } catch (error) {
      setState({ phase: 'error', received: 0, error: String((error as { detail?: string })?.detail || error) })
    } finally { setBusy(false) }
  }
  useEffect(() => {
    if (Date.now() - checkedAt > 6 * 60 * 60 * 1000) void check()
    else void invoke<UpdateState>('get_testing_update_state').then(setState).catch(console.error)
  }, [])
  useEffect(() => {
    if (state?.phase !== 'downloading') return
    const timer = setInterval(() => {
      void invoke<UpdateState>('get_testing_update_state').then(setState).catch(console.error)
    }, 1000)
    return () => clearInterval(timer)
  }, [state?.phase])
  return (
    <Box>
      <Button size="small" disabled={busy || state?.phase === 'downloading'} onClick={() => void check()}>
        {zh ? '检查测试版更新' : 'Check testing updates'}
      </Button>
      {state?.phase === 'unpublished' && <Typography variant="caption" sx={{ display: 'block' }} color="text.secondary">
        {zh ? '尚未发布自动下载源；当前安装包通过 GitHub Actions 分发。' : 'No automatic download published yet; installers are distributed through GitHub Actions.'}
      </Typography>}
      {state?.phase === 'current' && <Typography variant="caption">{zh ? '当前已是最新测试版' : 'Testing build is up to date'}</Typography>}
      {state?.phase === 'downloading' && <Box>
        <Typography variant="caption">{zh ? '后台下载中' : 'Downloading in background'} {state.version} · {(state.received / 1048576).toFixed(1)} MB</Typography>
        <LinearProgress variant={state.total ? 'determinate' : 'indeterminate'} value={state.total ? state.received / state.total * 100 : undefined} />
      </Box>}
      {state?.phase === 'downloaded' && <Alert severity="success">
        {zh ? `测试版 ${state.version} 已下载并通过 SHA-256 校验。请择时退出应用后安装。` : `Testing build ${state.version} downloaded and SHA-256 verified. Exit the app when ready to install.`}
        <Button size="small" onClick={() => void invoke('open_testing_update_folder').catch((error) => setState({ ...state, phase: 'error', error: String(error) }))}>
          {zh ? '打开下载目录' : 'Open download folder'}
        </Button>
      </Alert>}
      {state?.phase === 'error' && <Alert severity="warning">{state.error}</Alert>}
    </Box>
  )
}
