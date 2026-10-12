import { useEffect, useState } from 'react'
import type { AppInfo } from '@shared/api'
import { S } from '../lib/strings'
import { usePlayer } from '../stores/player'
import v from '../views/views.module.css'

export function AboutSection() {
  const api = usePlayer((p) => p.api)
  const engine = usePlayer((p) => p.engine)
  const [info, setInfo] = useState<AppInfo | null>(null)
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
      <div className={v.actions} style={{ marginTop: 12 }}>
        <button className={v.ghost} disabled={!info?.licenses} onClick={() => void api?.openLicenses()}>{S.thirdPartyLicenses}</button>
        <button className={v.ghost} onClick={() => void api?.openLogs()}>{S.openLogs}</button>
      </div>
      <p className="muted" style={{ margin: '12px 0 0', userSelect: 'text' }}>{S.licences}</p>
    </section>
  )
}
