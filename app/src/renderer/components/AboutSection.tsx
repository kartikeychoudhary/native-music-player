import { useEffect, useState } from 'react'
import type { AppInfo, UpdateState } from '@shared/api'
import { S } from '../lib/strings'
import { usePlayer } from '../stores/player'
import { useUi } from '../stores/ui'
import { useUpdates } from '../stores/updates'
import v from '../views/views.module.css'

function updateLine(s: UpdateState): string | null {
  switch (s.status) {
    case 'checking': return S.updateChecking
    case 'current': return S.updateCurrent
    case 'available': return S.updateAvailable(s.version)
    case 'downloading': return S.updateDownloading(s.version, s.percent)
    case 'ready': return S.updateReady(s.version)
    case 'error': return S.updateError(s.message)
    default: return null
  }
}

export function AboutSection() {
  const api = usePlayer((p) => p.api)
  const engine = usePlayer((p) => p.engine)
  const [info, setInfo] = useState<AppInfo | null>(null)
  const update = useUpdates((u) => u.state)
  const autoUpdate = useUi((u) => u.autoUpdate)
  useEffect(() => { void api?.appInfo().then(setInfo) }, [api])
  const ready = engine.state === 'ready' ? engine.engine : undefined
  return (
    <section className={v.section} aria-labelledby="about-title">
      <h2 id="about-title" className={v.sectionTitle}>{S.about}</h2>
      <div style={{ display: 'grid', gridTemplateColumns: '180px 1fr', gap: '12px 16px', alignItems: 'center' }}>
        <span className="muted">{S.versionLabel}</span><span className="mono">{info ? S.appVersion(info.version) : '…'}</span>
        <span className="muted">{S.engineLabel}</span><span className="mono">{ready ? S.engineVersion(ready.version) : S.engineNotRunning}</span>
        <span className="muted">{S.heAac}</span><span>{ready ? (ready.features.includes('fdk-aac') ? S.heAacIncluded : S.heAacMissing) : '…'}</span>
        <span className="muted">{S.buildLabel}</span><span className="mono">{info ? S.buildTarget(info.platform, info.arch, info.variant) : '…'}</span>
      </div>
      <h3 className={v.sectionTitle} style={{ marginTop: 20 }}>{S.updates}</h3>
      {info?.updateMode === 'off' && <p className="muted" style={{ margin: 0 }}>{S.updatesOff}</p>}
      {info && info.updateMode !== 'off' && (
        <>
          <label style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <input type="checkbox" checked={autoUpdate} onChange={(e) => useUi.getState().setAutoUpdate(e.target.checked)} /> {S.autoUpdate}
          </label>
          <div className={v.actions} style={{ marginTop: 12 }}>
            <button className={v.ghost} disabled={update.status === 'checking' || update.status === 'downloading'} onClick={() => void api?.checkForUpdates()}>{S.checkNow}</button>
            <span role="status">{updateLine(update)}</span>
          </div>
          {info.updateMode === 'notify' && <p className="muted" style={{ margin: '12px 0 0' }}>{S.updatesNotify}</p>}
        </>
      )}
      <div className={v.actions} style={{ marginTop: 12 }}>
        <button className={v.ghost} disabled={!info?.licenses} onClick={() => void api?.openLicenses()}>{S.thirdPartyLicenses}</button>
        <button className={v.ghost} onClick={() => void api?.openLogs()}>{S.openLogs}</button>
      </div>
      <p className="muted" style={{ margin: '12px 0 0', userSelect: 'text' }}>{S.licences}</p>
    </section>
  )
}
